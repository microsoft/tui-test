"""Private Python adapter for the shared, typed C ABI."""

import asyncio
import ctypes as C
from dataclasses import asdict, is_dataclass
import json
import os
from pathlib import Path
import sys
import threading

from . import _abi as abi


class NativeAssertionError(Exception):
    pass


class NativeUsageError(Exception):
    pass


class NativeNoSessionError(Exception):
    pass


class NativeInternalError(Exception):
    pass


_ERRORS = {
    1: ("assertion", NativeAssertionError),
    2: ("usage", NativeUsageError),
    3: ("no_session", NativeNoSessionError),
    5: ("internal", NativeInternalError),
}
_library = None
_library_lock = threading.Lock()


def _error(code, message, details=None, artifact=None):
    kind, cls = _ERRORS.get(code, _ERRORS[5])
    error = cls(message)
    error._tui_test_error_json = json.dumps(
        dict(kind=kind, message=message, details=details, artifact=artifact)
    )
    return error


def _load():
    global _library
    with _library_lock:
        if _library is None:
            filename = (
                "tui_test_ffi.dll" if sys.platform == "win32"
                else "libtui_test_ffi.dylib" if sys.platform == "darwin"
                else "libtui_test_ffi.so"
            )
            path = os.environ.get("TUI_TEST_NATIVE_LIBRARY")
            if not path:
                path = str(Path(__file__).with_name(filename))
            try:
                library = C.CDLL(path)
                abi.configure(library)
            except (OSError, AttributeError) as error:
                raise _error(5, "cannot load native library {!r}: {}".format(path, error)) from error
            version = library.tui_abi_version()
            if version != abi.ABI_VERSION:
                raise _error(5, "native ABI version mismatch: expected {}, got {}".format(
                    abi.ABI_VERSION, version
                ))
            _library = library
        return _library


def _integer(value, name, bits=64, signed=False):
    if isinstance(value, bool):
        raise _error(2, "{} must be an integer".format(name))
    try:
        value = value.__index__()
    except Exception:
        raise _error(2, "{} must be an integer".format(name)) from None
    if isinstance(value, bool) or not isinstance(value, int):
        raise _error(2, "{} must be an integer".format(name))
    minimum = -(1 << (bits - 1)) if signed else 0
    maximum = (1 << (bits - int(signed))) - 1
    if not minimum <= value <= maximum:
        raise _error(2, "{} must be an integer between {} and {}".format(name, minimum, maximum))
    return value


def _optional(value, name="timeout_ms", bits=64):
    return abi.TuiOptionalU64(
        value is not None, 0 if value is None else _integer(value, name, bits)
    )


def _boolean(value):
    return abi.TuiOptionalBool(value is not None, False if value is None else value)


def _float(value):
    return abi.TuiOptionalF64(value is not None, 0.0 if value is None else value)


def _string(value):
    if value is None:
        return abi.TuiString()
    if not isinstance(value, str):
        raise TypeError("expected a string")
    encoded = value.encode("utf-8")
    # The extra byte keeps an explicit empty string distinct from NULL.
    buffer = C.create_string_buffer(encoded)
    return abi.TuiString(C.cast(buffer, C.POINTER(C.c_uint8)), len(encoded))


def _strings(values):
    return (abi.TuiString * len(values))(*(_string(value) for value in values))


def _pairs(values):
    return (abi.TuiPair * len(values))(*(
        abi.TuiPair(_string(key), _string(value)) for key, value in values
    ))


def _text(value):
    if not value.data:
        return None
    return C.string_at(value.data, value.len).decode("utf-8")


def _copy(value):
    """Copy every borrowed field before the result allocation is released."""
    if isinstance(value, abi.TuiString):
        return _text(value)
    if isinstance(value, (abi.TuiOptionalU64, abi.TuiOptionalI32,
                          abi.TuiOptionalBool, abi.TuiOptionalF64)):
        return value.value if value.present else None
    if isinstance(value, abi.TuiColor):
        if value.kind == 0:
            return "default"
        if value.kind == 1:
            return value.index
        return "#{:02x}{:02x}{:02x}".format(value.red, value.green, value.blue)
    result = {}
    fields = {name for name, _ in value._fields_}
    for name, _ in value._fields_:
        if name.endswith("_len"):
            continue
        item = getattr(value, name)
        if name + "_len" in fields:
            item = [_copy(item[index]) for index in range(getattr(value, name + "_len"))]
        elif isinstance(item, C.Structure):
            item = _copy(item)
        result["char" if name == "character" else name] = item
    if isinstance(value, abi.TuiState):
        result["modes"] = {entry["name"]: entry["enabled"] for entry in result["modes"]}
    if isinstance(value, abi.TuiTerminalColors):
        result["palette"] = {int(entry["key"]): entry["value"] for entry in result["palette"]}
    return result


def _result(result, kind):
    if kind is None:
        return None
    if kind == "packed":
        return (memoryview(C.string_at(result.text.data, result.text.len)),
                result.size.cols, result.size.rows)
    if kind in ("cells", "matches", "bells", "strings", "modes"):
        values = getattr(result, kind)
        copied = [_copy(values[index]) for index in range(getattr(result, kind + "_len"))]
        return {entry["name"]: entry["enabled"] for entry in copied} if kind == "modes" else copied
    if kind == "snapshot":
        return ("passed", "written", "updated")[result.snapshot]
    value = getattr(result, kind)
    return _copy(value) if isinstance(value, C.Structure) else value


def _invoke(name, args, kind=None):
    library = _load()
    pointer = getattr(library, "tui_" + name)(*args)
    if not pointer:
        raise _error(5, "native operation returned a null result")
    try:
        result = pointer.contents
        if result.error_kind:
            details = _text(result.error_details)
            artifact = _text(result.error_artifact)
            raise _error(
                result.error_kind, _text(result.error_message),
                json.loads(details) if details is not None else None,
                json.loads(artifact) if artifact is not None else None,
            )
        return _result(result, kind)
    finally:
        library.tui_result_free(pointer)


async def _background(function):
    loop = asyncio.get_running_loop()
    future = loop.create_future()

    def deliver(value, error):
        if not future.done():
            if error is None:
                future.set_result(value)
            else:
                future.set_exception(error)

    def worker():
        value, error = None, None
        try:
            value = function()
        except Exception as caught:
            error = caught
        try:
            loop.call_soon_threadsafe(deliver, value, error)
        except RuntimeError:
            pass  # The event loop can close before a cancelled wait returns.

    # A blocked wait must never consume the last worker needed by close().
    threading.Thread(target=worker, name="tui-test-ffi", daemon=True).start()
    return await future


def _wait(timeout_ms, regex=False, not_=False):
    return abi.TuiWaitOptions(_optional(timeout_ms), regex, not_)


def _mouse(button):
    # The existing Python API uses terminal button codes, including modifiers.
    code = _integer(button, "button", 8)
    if code & ~31 or code & 3 == 3:
        raise _error(2, "invalid mouse button code {}".format(code))
    return abi.TuiMouseOptions(code & 3, bool(code & 8), bool(code & 16), bool(code & 4))


def _query(expression):
    expression = dict(expression)
    nodes = []
    for source in expression["nodes"]:
        node = dict(source)
        nth = node.pop("nth", None)
        occurrence = node.get("occurrence", "any")
        if occurrence == "nth":
            occurrence = {"nth": _integer(nth, "nth", C.sizeof(C.c_size_t) * 8)}
        node["occurrence"] = occurrence
        nodes.append(node)
    expression["nodes"] = nodes
    return abi.TuiQuery(_string(json.dumps(expression)))

def _style(value, cls=abi.TuiStyle):
    if value is None:
        return cls()
    if is_dataclass(value):
        value = asdict(value)
    if cls is abi.TuiPadding:
        if isinstance(value, int):
            return cls(uniform=_optional(value, "padding", 32))
        return cls(sides=_style(value, abi.TuiPaddingSides))
    if not isinstance(value, dict):
        raise TypeError("rendering style must be a dataclass or dictionary")
    fields = dict(cls._fields_)
    for key in value:
        if key not in fields or key.endswith("_len"):
            raise TypeError("unknown rendering style field: " + key)
    result = cls()
    for key, item in value.items():
        if item is None:
            continue
        field = fields[key]
        if field is abi.TuiString:
            converted = _string(item)
        elif field is abi.TuiOptionalU64:
            converted = _optional(item, key)
        elif field is abi.TuiOptionalBool:
            if not isinstance(item, bool):
                raise TypeError(key + " must be a boolean")
            converted = _boolean(item)
        elif field is abi.TuiOptionalF64:
            if isinstance(item, bool) or not isinstance(item, (int, float)):
                raise TypeError(key + " must be a number")
            converted = _float(item)
        elif key == "files":
            if not isinstance(item, (list, tuple)):
                raise TypeError("font files must be a sequence")
            converted = _strings(item)
            result.files_len = len(item)
        else:
            converted = _style(item, field)
        setattr(result, key, converted)
    return result


class NativeSession:
    def __init__(self, name, recording_directory=None, artifact_directory=None,
                 artifact_mode=None, artifact_include_recording=False,
                 trace_mode=None, trace_directory=None):
        self.name = name
        self._recording_directory = recording_directory
        self._context = dict(
            name=name, artifact_directory=artifact_directory, artifact_mode=artifact_mode,
            artifact_include_recording=artifact_include_recording,
            trace_mode=trace_mode, trace_directory=trace_directory,
        )

    def _session(self, screen_history_limit=None):
        context = dict(self._context)
        if screen_history_limit is not None:
            context["screen_history_limit"] = _integer(
                screen_history_limit, "screen_history_limit", 16
            )
        return _string(json.dumps(context))

    async def _call(self, name, *args, **kwargs):
        return await _background(
            lambda: _invoke(name, (self._session(),) + args, kwargs.get("kind"))
        )

    def _open_options(self, shell, backend, cols, rows, cwd, env, wait_ready,
                      restart, profile_scrollback, profile_colors, text_timeout,
                      idle_timeout, command_timeout, exit_timeout, ready_timeout, style=None):
        return abi.TuiOpenOptions(
            backend=_string(backend), shell=_string(shell),
            cols=_optional(_integer(cols, "cols", 16)), rows=_optional(_integer(rows, "rows", 16)),
            cwd=_string(cwd), env=_pairs(env), env_len=len(env), wait_ready=_boolean(wait_ready), restart=restart,
            scrollback=_optional(profile_scrollback, "scrollback"), colors=_pairs(profile_colors),
            colors_len=len(profile_colors), timeouts=abi.TuiTimeouts(*(
                _optional(value, name + "_timeout") for name, value in zip(
                    ("text", "idle", "command", "exit", "ready"),
                    (text_timeout, idle_timeout, command_timeout, exit_timeout, ready_timeout)
                )
            )), recording_directory=_string(self._recording_directory), style=_style(style),
        )

    async def open(self, shell, backend, cols, rows, cwd, env, wait_ready, restart,
                   profile_scrollback, profile_colors, text_timeout, idle_timeout,
                   command_timeout, exit_timeout, ready_timeout, screen_history_limit, style=None):
        options = self._open_options(
            shell, backend, cols, rows, cwd, env, wait_ready, restart,
            profile_scrollback, profile_colors, text_timeout, idle_timeout,
            command_timeout, exit_timeout, ready_timeout, style,
        )
        session = self._session(screen_history_limit)
        return await _background(
            lambda: _invoke("open_ptr", (session, C.byref(options)), "open"),
        )

    async def run(self, program, args, backend, cols, rows, cwd, env, wait_ready, restart,
                  profile_scrollback, profile_colors, text_timeout, idle_timeout,
                  command_timeout, exit_timeout, ready_timeout, screen_history_limit, style=None):
        options = self._open_options(
            None, backend, cols, rows, cwd, env, wait_ready, restart,
            profile_scrollback, profile_colors, text_timeout, idle_timeout,
            command_timeout, exit_timeout, ready_timeout, style,
        )
        session = self._session(screen_history_limit)
        arguments = (session, C.byref(options), _string(program), _strings(args), len(args))
        return await _background(
            lambda: _invoke("run_ptr", arguments, "open"),
        )

    async def restart(self, graceful_timeout_ms):
        timeout = _integer(graceful_timeout_ms, "graceful_timeout_ms")
        session = self._session()
        return await _background(
            lambda: _invoke("restart", (session, timeout), "open"),
        )

    async def close(self, failed=None):
        return await self._call("close", _boolean(failed))

    async def state(self):
        return await self._call("state", kind="state")

    async def get_modes(self):
        return await self._call("get_modes", kind="modes")

    async def get_colors(self):
        return await self._call("get_colors", kind="colors")

    async def expect_mode(self, mode, enabled, timeout):
        if not isinstance(enabled, bool):
            raise TypeError("enabled must be a boolean")
        return await self._call("expect_mode", _string(mode), enabled, _optional(timeout))

    async def expect_cursor(self, x, y, visible, shape, timeout):
        if visible is not None and not isinstance(visible, bool):
            raise TypeError("visible must be a boolean")
        options = abi.TuiCursorExpectation(
            x=_optional(x, "x", 16), y=_optional(y, "y", 16), visible=_boolean(visible),
            shape=_string(shape), timeout_ms=_optional(timeout),
        )
        return await self._call("expect_cursor", options)

    async def expect_colors(self, foreground, background, cursor, palette, timeout):
        pairs = [(str(_integer(index, "palette index", 8)), color) for index, color in palette.items()]
        options = abi.TuiColorExpectation(
            foreground=_string(foreground), background=_string(background), cursor=_string(cursor),
            palette=_pairs(pairs), palette_len=len(pairs), timeout_ms=_optional(timeout),
        )
        return await self._call("expect_colors", options)

    async def text(self, full):
        return await self._call("text", full, kind="text")

    async def packed_screen(self, full):
        return await self._call("packed_screen", full, kind="packed")

    async def cells(self, x, y, w, h):
        return await self._call("cells", *(
            _integer(value, name, 16) for name, value in zip(("x", "y", "w", "h"), (x, y, w, h))
        ), kind="cells")

    async def find_locator(self, expression, require_one):
        return await self._call("find_locator", _query(expression), require_one, kind="matches")

    async def wait_locator(self, expression, not_, timeout_ms):
        return await self._call("wait_locator", _query(expression), _wait(timeout_ms, not_=not_))

    async def expect_locator(self, expression, not_, timeout_ms):
        return await self._call("expect_locator", _query(expression), _wait(timeout_ms, not_=not_))

    async def click_locator(self, expression, button, clicks, timeout_ms):
        return await self._call("click_locator", _query(expression), _mouse(button),
                                _integer(clicks, "clicks", 8), _optional(timeout_ms))

    async def highlight_locator(self, expression, timeout_ms):
        return await self._call("highlight_locator", _query(expression), _optional(timeout_ms),
                                kind="matches")

    async def get_command(self):
        return await self._call("get_command", kind="text")

    async def get_output(self):
        return await self._call("get_output", kind="text")

    async def get_exit_code(self):
        return await self._call("get_exit_code", kind="exit_code")

    async def get_cwd(self):
        return await self._call("get_cwd", kind="text")

    async def get_title(self):
        return await self._call("get_title", kind="text")

    async def get_clipboard(self):
        return await self._call("get_clipboard", kind="text")

    async def get_cursor(self):
        return await self._call("get_cursor", kind="cursor")

    async def get_size(self):
        return await self._call("get_size", kind="size")

    async def get_viewport(self):
        return await self._call("get_viewport", kind="viewport")

    async def scroll(self, lines):
        return await self._call("scroll", _integer(lines, "lines", 32, signed=True))

    async def scroll_to_top(self):
        return await self._call("scroll_to_top")

    async def scroll_to_bottom(self):
        return await self._call("scroll_to_bottom")

    async def get_bell_count(self):
        return await self._call("get_bell_count", kind="number")

    async def get_bell_events(self):
        return await self._call("get_bell_events", kind="bells")

    async def write(self, data):
        return await self._call("write", _string(data))

    async def type(self, text):
        return await self._call("type", _string(text))

    async def submit(self, data):
        return await self._call("submit", _string(data))

    async def _key(self, keys, action):
        return await self._call("key", _strings(keys), len(keys), action)

    async def press(self, keys):
        return await self._key(keys, 0)

    async def key_down(self, keys):
        return await self._key(keys, 1)

    async def repeat(self, keys):
        return await self._key(keys, 2)

    async def key_up(self, keys):
        return await self._key(keys, 3)

    async def mouse_click(self, x, y, on_text, button, clicks):
        return await self._call("mouse_click", _optional(x, "x", 16), _optional(y, "y", 16),
                                _string(on_text), _mouse(button), _integer(clicks, "clicks", 8))

    async def mouse_move(self, x, y):
        return await self._call("mouse_move", _integer(x, "x", 16), _integer(y, "y", 16))

    async def mouse_down(self, x, y, button):
        return await self._call("mouse_down", _integer(x, "x", 16), _integer(y, "y", 16),
                                _mouse(button))

    async def mouse_up(self, x, y, button):
        return await self._call("mouse_up", _integer(x, "x", 16), _integer(y, "y", 16),
                                _mouse(button))

    async def mouse_drag(self, x1, y1, x2, y2, button):
        coordinates = [_integer(value, name, 16) for name, value in zip(
            ("x1", "y1", "x2", "y2"), (x1, y1, x2, y2)
        )]
        return await self._call("mouse_drag", *coordinates, _mouse(button))

    async def mouse_scroll(self, direction, amount):
        return await self._call("mouse_scroll", _string(direction), _integer(amount, "amount", 16))

    async def resize(self, cols, rows):
        return await self._call("resize", _integer(cols, "cols", 16), _integer(rows, "rows", 16))

    async def signal(self, signal):
        return await self._call("signal", _string(signal))

    async def kill(self):
        return await self.signal("KILL")

    async def wait_title(self, text, regex, not_, timeout_ms):
        return await self._call("wait_title", _string(text), _wait(timeout_ms, regex, not_))

    async def wait_clipboard(self, text, regex, timeout_ms):
        return await self._call("wait_clipboard", _string(text), _wait(timeout_ms, regex))

    async def wait_idle(self, timeout_ms):
        return await self._call("wait_idle", _optional(timeout_ms))

    async def wait_command(self, timeout_ms):
        return await self._call("wait_command", _optional(timeout_ms))

    async def wait_exit(self, timeout_ms):
        return await self._call("wait_exit", _optional(timeout_ms))

    async def wait_ready(self, timeout_ms):
        return await self._call("wait_ready", _optional(timeout_ms))

    async def wait_bell(self, timeout_ms):
        return await self._call("wait_bell", _optional(timeout_ms))

    async def expect_title(self, text, regex, not_, timeout_ms):
        return await self._call("expect_title", _string(text), _wait(timeout_ms, regex, not_))

    async def expect_exit_code(self, code, timeout_ms):
        return await self._call("expect_exit_code", _integer(code, "code", 32, signed=True),
                                _optional(timeout_ms))

    async def expect_output(self, text, regex):
        return await self._call("expect_output", _string(text), regex)

    async def expect_bell_count(self, count, timeout_ms):
        return await self._call("expect_bell_count", _integer(count, "count"), _optional(timeout_ms))

    async def snapshot(self, name, update, include_style, include_title, cwd):
        return await self._call("snapshot", _string(name), update, include_style,
                                include_title, _string(cwd), kind="snapshot")

    async def screenshot(self, path, full, zoom=None, background=None, transparent=False):
        return await self._call("screenshot", full, _string(path), _float(zoom),
                                _string(background), transparent, kind="text")

    async def start_recording(self, path, format, fps, speed, idle_time_limit, zoom=None,
                              background=None, transparent=False):
        options = abi.TuiRecordingOptions(
            _string(path), _string(format), _optional(fps, "fps", 8), _float(speed),
            _float(idle_time_limit), _float(zoom), _string(background), transparent,
        )
        return await self._call("start_recording", options)

    async def stop_recording(self):
        return await self._call("stop_recording", kind="text")

    async def pause_recording(self):
        return await self._call("pause_recording")

    async def resume_recording(self):
        return await self._call("resume_recording")

    async def recording(self):
        return await recording(self.name)


async def sessions():
    return await _background(lambda: _invoke("sessions", (), "strings"))


async def close_all():
    return await _background(lambda: _invoke("close_all", ()))


def _close_all_blocking():
    if _library is not None:
        _invoke("close_all", ())


async def recording(name):
    return await _background(lambda: _invoke("recording", (_string(name),), "text"))
