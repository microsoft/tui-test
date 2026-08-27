from __future__ import annotations

import atexit
import copy
import json
import os
import time
from dataclasses import asdict
from typing import (
    Any,
    Awaitable,
    Callable,
    Dict,
    Iterable,
    Literal,
    List,
    Mapping,
    Optional,
    Tuple,
    TypeVar,
    Union,
)

from . import _config as cfg
from . import _ephemeral as ephemeral
from . import _native as native
from .errors import (
    ExpectationError,
    InternalError,
    NoSessionError,
    TerminalArtifact,
    TuiTestError,
    UsageError,
)
from .types import (
    Backend,
    BellEvent,
    Cell,
    Profile,
    RecordingFormat,
    State,
    TextAnchor,
    TextMatch,
    TextOccurrence,
    TextStyle,
    Timeouts,
)

_TERMINAL_MARKER = "Terminal content:\n"
_TIMEOUT_CLASSES = ("text", "idle", "command", "exit", "ready")

_T = TypeVar("_T")
EnvLike = Union[Mapping[str, str], Iterable[Tuple[str, str]], None]


async def _await_native(awaitable: Awaitable[_T]) -> _T:
    try:
        return await awaitable
    except native.NativeAssertionError as error:
        raise ExpectationError(str(error)) from error
    except native.NativeUsageError as error:
        raise UsageError(str(error)) from error
    except native.NativeNoSessionError as error:
        raise NoSessionError(str(error)) from error
    except native.NativeInternalError as error:
        raise InternalError(str(error)) from error


def _atexit_close_all() -> None:
    try:
        native._close_all_blocking()
    except Exception:
        pass


atexit.register(_atexit_close_all)


def _env_pairs(env: EnvLike) -> List[Tuple[str, str]]:
    if env is None:
        return []
    items = env.items() if isinstance(env, Mapping) else env
    return [(str(key), str(value)) for key, value in items]


def _session_timeout_values(timeouts: object) -> Tuple[Optional[int], ...]:
    normalized = cfg.session_timeouts_payload(timeouts) or {}
    return tuple(normalized.get(class_name) for class_name in _TIMEOUT_CLASSES)


def _profile_values(
    profile: object,
) -> Tuple[Optional[int], List[Tuple[str, str]]]:
    normalized = cfg.normalize_profile(profile) or {}
    colors = normalized.get("colors") or {}
    return normalized.get("scrollback"), list(colors.items())


def _occurrence_value(value: TextOccurrence) -> object:
    if isinstance(value, int) and not isinstance(value, bool):
        if value < 0:
            raise ValueError("text occurrence index must be non-negative")
        return {"nth": value}
    if value not in ("any", "unique", "first", "last"):
        raise ValueError(
            "text occurrence must be any, unique, first, last, or a non-negative integer"
        )
    return value


def _anchor_value(anchor: Optional[TextAnchor]) -> Optional[Dict[str, object]]:
    if anchor is None:
        return None
    return {
        "text": anchor.text,
        "regex": anchor.regex,
        "occurrence": _occurrence_value(anchor.occurrence),
    }


def _selector_value(
    text: str,
    *,
    regex: bool,
    full: bool,
    whitespace: str,
    after: Optional[TextAnchor],
    before: Optional[TextAnchor],
    occurrence: TextOccurrence,
) -> Dict[str, object]:
    return {
        "text": text,
        "regex": regex,
        "full": full,
        "whitespace": whitespace,
        "scope": {
            "after": _anchor_value(after),
            "before": _anchor_value(before),
        },
        "occurrence": _occurrence_value(occurrence),
    }


def _text_query_value(
    text: str,
    *,
    regex: bool,
    full: bool,
    whitespace: str,
    after: Optional[TextAnchor],
    before: Optional[TextAnchor],
    occurrence: TextOccurrence,
    within: Optional[Dict[str, object]],
) -> Dict[str, object]:
    return {
        "selector": {
            "kind": "text",
            "selector": _selector_value(
                text,
                regex=regex,
                full=full,
                whitespace=whitespace,
                after=after,
                before=before,
                occurrence=occurrence,
            ),
        },
        "within": copy.deepcopy(within),
    }


def _style_query_value(
    style: TextStyle,
    *,
    full: bool,
    occurrence: TextOccurrence,
    within: Optional[Dict[str, object]],
) -> Dict[str, object]:
    style_value = asdict(style)
    if not any(value is not None for value in style_value.values()):
        raise ValueError("get_by_style requires at least one style property")
    return {
        "selector": {
            "kind": "style",
            "selector": {
                "style": style_value,
                "full": full,
                "occurrence": _occurrence_value(occurrence),
            },
        },
        "within": copy.deepcopy(within),
    }


def _extract_terminal_text(message: Optional[str]) -> Optional[str]:
    if not message:
        return None
    index = message.find(_TERMINAL_MARKER)
    if index == -1:
        return None
    return message[index + len(_TERMINAL_MARKER):].rstrip("\n") or None


class _Keyboard:
    def __init__(self, client: "TuiTest") -> None:
        self._c = client

    async def press(self, *keys: str) -> None:
        await self._c._await(self._c._native.press(list(keys)))

    async def down(self, *keys: str) -> None:
        await self._c._await(self._c._native.key_down(list(keys)))

    async def repeat(self, *keys: str) -> None:
        await self._c._await(self._c._native.repeat(list(keys)))

    async def up(self, *keys: str) -> None:
        await self._c._await(self._c._native.key_up(list(keys)))


class _Mouse:
    def __init__(self, client: "TuiTest") -> None:
        self._c = client

    async def click(
        self,
        x: Optional[int] = None,
        y: Optional[int] = None,
        *,
        on_text: Optional[str] = None,
        button: int = 0,
        clicks: int = 1,
    ) -> None:
        await self._c._await(
            self._c._native.mouse_click(x, y, on_text, button, clicks)
        )

    async def move(self, x: int, y: int) -> None:
        await self._c._await(self._c._native.mouse_move(x, y))

    async def down(self, x: int, y: int, *, button: int = 0) -> None:
        await self._c._await(self._c._native.mouse_down(x, y, button))

    async def up(self, x: int, y: int, *, button: int = 0) -> None:
        await self._c._await(self._c._native.mouse_up(x, y, button))

    async def drag(
        self, x1: int, y1: int, x2: int, y2: int, *, button: int = 0
    ) -> None:
        await self._c._await(
            self._c._native.mouse_drag(x1, y1, x2, y2, button)
        )

    async def scroll(self, direction: str, *, amount: int = 3) -> None:
        await self._c._await(self._c._native.mouse_scroll(direction, amount))


class Locator:
    """A lazy text/style query resolved against the current terminal grid."""

    def __init__(
        self, client: "TuiTest", query: Dict[str, object]
    ) -> None:
        self._client = client
        self._query = copy.deepcopy(query)

    def _with_occurrence(self, occurrence: TextOccurrence) -> "Locator":
        query = copy.deepcopy(self._query)
        query["selector"]["selector"]["occurrence"] = _occurrence_value(
            occurrence
        )
        return Locator(self._client, query)

    def _strict_query(self) -> Dict[str, object]:
        query = copy.deepcopy(self._query)
        selector = query["selector"]["selector"]
        if selector["occurrence"] == "any":
            selector["occurrence"] = "unique"
        return query

    def any(self) -> "Locator":
        return self._with_occurrence("any")

    def unique(self) -> "Locator":
        return self._with_occurrence("unique")

    def first(self) -> "Locator":
        return self._with_occurrence("first")

    def last(self) -> "Locator":
        return self._with_occurrence("last")

    def nth(self, index: int) -> "Locator":
        if isinstance(index, bool) or not isinstance(index, int) or index < 0:
            raise ValueError("locator nth index must be a non-negative integer")
        return self._with_occurrence(index)

    def get_by_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        whitespace: str = "exact",
        after: Optional[TextAnchor] = None,
        before: Optional[TextAnchor] = None,
        occurrence: TextOccurrence = "any",
    ) -> "Locator":
        return self._client._make_text_locator(
            text,
            regex=regex,
            full=full,
            whitespace=whitespace,
            after=after,
            before=before,
            occurrence=occurrence,
            within=self._query,
        )

    def get_by_style(
        self,
        style: TextStyle,
        *,
        full: bool = False,
        occurrence: TextOccurrence = "any",
    ) -> "Locator":
        return self._client._make_style_locator(
            style,
            full=full,
            occurrence=occurrence,
            within=self._query,
        )

    async def locations(self) -> List[TextMatch]:
        return await self._locations("locator.locations")

    async def _locations(self, op_name: str) -> List[TextMatch]:
        values = await self._client._guarded(
            op_name,
            self._client._native.find_locator(json.dumps(self._query)),
        )
        return [TextMatch.from_dict(value) for value in values]

    async def location(self) -> TextMatch:
        query = self._strict_query()
        values = await self._client._guarded(
            "locator.location",
            self._client._native.find_locator(json.dumps(query)),
        )
        if len(values) != 1:
            current = self._query["selector"]
            description = (
                repr(current["selector"]["text"])
                if current["kind"] == "text"
                else "style"
            )
            message = "locator.location: no match found for {}".format(
                description
            )
            try:
                message += "\n\nTerminal content:\n{}".format(
                    await self._client.text()
                )
            except TuiTestError as diagnostic_error:
                message += "\n\nTerminal content unavailable: {}".format(
                    diagnostic_error
                )
            error = ExpectationError(message)
            await self._client._capture_artifacts(error)
            raise error
        return TextMatch.from_dict(values[0])

    async def count(self) -> int:
        return len(await self.locations())

    async def all(self) -> List["Locator"]:
        matches = await self.locations()
        if (
            self._query["selector"]["selector"]["occurrence"]
            == "any"
        ):
            return [self.nth(index) for index in range(len(matches))]
        return [Locator(self._client, self._query) for _ in matches]

    async def wait(
        self,
        *,
        state: Literal["visible", "hidden"] = "visible",
        timeout: Optional[int] = None,
    ) -> "Locator":
        return await self._wait(state=state, timeout=timeout, op_name="locator.wait")

    async def _wait(
        self,
        *,
        state: Literal["visible", "hidden"],
        timeout: Optional[int],
        op_name: str,
    ) -> "Locator":
        if state not in ("visible", "hidden"):
            raise ValueError("locator state must be 'visible' or 'hidden'")
        await self._client._guarded(
            op_name,
            self._client._native.wait_locator(
                json.dumps(self._query),
                state == "hidden",
                self._client._timeout("text", timeout),
            ),
        )
        return self

    async def click(
        self,
        *,
        button: int = 0,
        clicks: int = 1,
        timeout: Optional[int] = None,
    ) -> None:
        await self._client._guarded(
            "locator.click",
            self._client._native.click_locator(
                json.dumps(self._strict_query()),
                button,
                clicks,
                self._client._timeout("text", timeout),
            ),
        )

    async def highlight(self, *, timeout: Optional[int] = None) -> None:
        await self._client._guarded(
            "locator.highlight",
            self._client._native.highlight_locator(
                json.dumps(self._query),
                self._client._timeout("text", timeout),
            ),
        )

    async def expect(
        self,
        *,
        not_: bool = False,
        style: Optional[TextStyle] = None,
        timeout: Optional[int] = None,
    ) -> None:
        await self._expect(
            not_=not_,
            style=style,
            timeout=timeout,
            op_name="locator.expect",
        )

    async def _expect(
        self,
        *,
        not_: bool,
        style: Optional[TextStyle],
        timeout: Optional[int],
        op_name: str,
    ) -> None:
        await self._client._guarded(
            op_name,
            self._client._native.expect_locator(
                json.dumps(
                    [
                        self._query,
                        asdict(style or TextStyle()),
                        not_,
                    ]
                ),
                self._client._timeout("text", timeout),
            ),
        )


class TuiTest:
    def __init__(
        self,
        session: Optional[str] = None,
        *,
        backend: Optional[Backend] = None,
        timeouts: Optional[Timeouts] = None,
        profile: Optional[Profile] = None,
        artifacts: Optional[Dict[str, Any]] = None,
    ) -> None:
        self._session = cfg.resolve_session(session)
        self._native = native.NativeSession(self._session)
        self._backend = cfg.normalize_backend(backend)
        self._timeouts = cfg.normalize_timeouts(timeouts)
        self._profile = cfg.normalize_profile(profile)
        self._artifacts = artifacts
        self._artifact_counter = 0
        self.keyboard = _Keyboard(self)
        self.mouse = _Mouse(self)

    @classmethod
    def ephemeral(cls, prefix: Optional[str] = None, **kwargs: Any) -> "TuiTest":
        return cls(ephemeral.unique_session(prefix), **kwargs)

    @property
    def session(self) -> str:
        return self._session

    def _timeout(self, class_name: str, call: Optional[int]) -> Optional[int]:
        return cfg.resolve_timeout(
            class_name, call=call, timeouts=self._timeouts
        )

    async def _await(self, awaitable: Awaitable[_T]) -> _T:
        return await _await_native(awaitable)

    async def _guarded(self, op_name: str, awaitable: Awaitable[_T]) -> _T:
        try:
            return await self._await(awaitable)
        except ExpectationError as error:
            error.message = f"{op_name}: {error.message}"
            error.args = (error.message,)
            await self._capture_artifacts(error)
            raise

    async def _capture_artifacts(self, error: ExpectationError) -> None:
        artifacts = self._artifacts
        if artifacts is None:
            return
        mode = artifacts.get("on_failure", "svg")
        if mode == "none":
            return
        text = None  # type: Optional[str]
        screenshot_path = None  # type: Optional[str]
        try:
            text = _extract_terminal_text(error.message)
        except Exception:
            pass
        if mode == "svg":
            try:
                screenshot_path = await self._write_artifact_svg()
            except Exception:
                pass
        if text is None and screenshot_path is None:
            return
        try:
            error.terminal = TerminalArtifact(text=text, screenshot=screenshot_path)
        except Exception:
            pass

    async def _write_artifact_svg(self) -> Optional[str]:
        directory = self._artifacts.get("dir") if self._artifacts else None
        if not directory:
            return None
        os.makedirs(directory, exist_ok=True)
        self._artifact_counter += 1
        timestamp = time.strftime("%Y%m%d-%H%M%S")
        filename = "{}-{}-{}.svg".format(
            self._session, timestamp, self._artifact_counter
        )
        path = os.path.join(directory, filename)
        await self.screenshot(path)
        return path

    async def _spawn(
        self,
        start: Callable[[], Awaitable[Dict[str, Any]]],
        retries: int,
    ) -> Dict[str, Any]:
        attempts = retries + 1 if retries > 0 else 1
        for attempt in range(attempts):
            try:
                return await self._await(start())
            except Exception:
                if attempt + 1 < attempts:
                    await self.close_quiet()
                else:
                    raise
        raise AssertionError("unreachable")

    async def open(
        self,
        *,
        shell: Optional[str] = None,
        backend: Optional[Backend] = None,
        cols: int = cfg.DEFAULT_COLS,
        rows: int = cfg.DEFAULT_ROWS,
        cwd: Optional[str] = None,
        env: EnvLike = None,
        wait_ready: Optional[bool] = None,
        restart: bool = False,
        profile: Optional[Profile] = None,
        timeouts: Optional[Timeouts] = None,
        retries: int = 0,
    ) -> Dict[str, Any]:
        env_values = _env_pairs(env)
        profile_values = _profile_values(
            profile if profile is not None else self._profile
        )
        timeout_values = _session_timeout_values(timeouts)
        return await self._spawn(
            lambda: self._native.open(
                shell,
                cfg.normalize_backend(
                    backend if backend is not None else self._backend
                ),
                cols,
                rows,
                cwd,
                env_values,
                wait_ready,
                restart,
                *profile_values,
                *timeout_values,
            ),
            retries,
        )

    async def run(
        self,
        program: str,
        *args: str,
        backend: Optional[Backend] = None,
        cols: int = cfg.DEFAULT_COLS,
        rows: int = cfg.DEFAULT_ROWS,
        cwd: Optional[str] = None,
        env: EnvLike = None,
        wait_ready: Optional[bool] = None,
        restart: bool = False,
        profile: Optional[Profile] = None,
        timeouts: Optional[Timeouts] = None,
        retries: int = 0,
    ) -> Dict[str, Any]:
        env_values = _env_pairs(env)
        profile_values = _profile_values(
            profile if profile is not None else self._profile
        )
        timeout_values = _session_timeout_values(timeouts)
        return await self._spawn(
            lambda: self._native.run(
                program,
                list(args),
                cfg.normalize_backend(
                    backend if backend is not None else self._backend
                ),
                cols,
                rows,
                cwd,
                env_values,
                wait_ready,
                restart,
                *profile_values,
                *timeout_values,
            ),
            retries,
        )

    async def close(self) -> None:
        await self._await(self._native.close())

    async def close_quiet(self) -> None:
        try:
            await self.close()
        except Exception:
            pass

    async def type(self, text: str) -> None:
        await self._await(self._native.type(text))

    async def write(self, data: str) -> None:
        await self._await(self._native.write(data))

    async def submit(self, text: Optional[str] = None) -> None:
        await self._await(self._native.submit(text))

    async def press(self, *keys: str) -> None:
        await self.keyboard.press(*keys)

    async def resize(self, cols: int, rows: int) -> None:
        await self._await(self._native.resize(cols, rows))

    async def signal(self, name: str) -> None:
        await self._await(self._native.signal(name))

    async def kill(self) -> None:
        await self._await(self._native.kill())

    async def state(self) -> State:
        return State.from_dict(await self._await(self._native.state()))

    async def text(self, *, full: bool = False) -> str:
        return await self._await(self._native.text(full))

    def get_by_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        whitespace: str = "exact",
        after: Optional[TextAnchor] = None,
        before: Optional[TextAnchor] = None,
        occurrence: TextOccurrence = "any",
    ) -> Locator:
        return self._make_text_locator(
            text,
            regex=regex,
            full=full,
            whitespace=whitespace,
            after=after,
            before=before,
            occurrence=occurrence,
            within=None,
        )

    def _make_text_locator(
        self,
        text: str,
        *,
        regex: bool,
        full: bool,
        whitespace: str,
        after: Optional[TextAnchor],
        before: Optional[TextAnchor],
        occurrence: TextOccurrence,
        within: Optional[Dict[str, object]],
    ) -> Locator:
        return Locator(
            self,
            _text_query_value(
                text,
                regex=regex,
                full=full,
                whitespace=whitespace,
                after=after,
                before=before,
                occurrence=occurrence,
                within=within,
            ),
        )

    def get_by_style(
        self,
        style: TextStyle,
        *,
        full: bool = False,
        occurrence: TextOccurrence = "any",
    ) -> Locator:
        return self._make_style_locator(
            style,
            full=full,
            occurrence=occurrence,
            within=None,
        )

    def _make_style_locator(
        self,
        style: TextStyle,
        *,
        full: bool,
        occurrence: TextOccurrence,
        within: Optional[Dict[str, object]],
    ) -> Locator:
        return Locator(
            self,
            _style_query_value(
                style,
                full=full,
                occurrence=occurrence,
                within=within,
            ),
        )

    async def find_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        whitespace: str = "exact",
        after: Optional[TextAnchor] = None,
        before: Optional[TextAnchor] = None,
        occurrence: TextOccurrence = "any",
    ) -> List[TextMatch]:
        return await self.get_by_text(
            text,
            regex=regex,
            full=full,
            whitespace=whitespace,
            after=after,
            before=before,
            occurrence=occurrence,
        )._locations("find_text")

    async def _packed_screen(
        self, *, full: bool = False
    ) -> Tuple[memoryview, int, int]:
        """Return owned UTF-8 logical rows and terminal cell dimensions."""
        return await self._await(self._native.packed_screen(full))

    async def cells(self, x: int, y: int, w: int = 1, h: int = 1) -> List[Cell]:
        data = await self._await(self._native.cells(x, y, w, h))
        return [Cell(**cell) for cell in data]

    async def get_command(self) -> Optional[str]:
        return await self._await(self._native.get_command())

    async def get_output(self) -> Optional[str]:
        return await self._await(self._native.get_output())

    async def get_exit_code(self) -> Optional[int]:
        return await self._await(self._native.get_exit_code())

    async def get_cwd(self) -> Optional[str]:
        return await self._await(self._native.get_cwd())

    async def get_title(self) -> Optional[str]:
        return await self._await(self._native.get_title())

    async def get_cursor(self) -> Dict[str, int]:
        return await self._await(self._native.get_cursor())

    async def get_size(self) -> Dict[str, int]:
        return await self._await(self._native.get_size())

    async def get_bell_count(self) -> int:
        return await self._await(self._native.get_bell_count())

    async def get_bell_events(self) -> List[BellEvent]:
        events = await self._await(self._native.get_bell_events())
        return [
            BellEvent(
                sequence=event.get("sequence", 0),
                elapsed_ms=event.get("elapsed_ms", 0),
            )
            for event in events
        ]

    async def screenshot(
        self,
        path: Optional[str] = None,
        *,
        full: bool = False,
        zoom: Optional[float] = None,
    ) -> str:
        if zoom is not None and path is None:
            raise ValueError("screenshot zoom requires a path")
        return await self._await(self._native.screenshot(path, full, zoom))

    async def start_recording(
        self,
        path: str,
        *,
        format: Optional[RecordingFormat] = None,
        fps: Optional[int] = None,
        speed: Optional[float] = None,
        idle_time_limit: Optional[float] = None,
        zoom: Optional[float] = None,
    ) -> None:
        await self._await(
            self._native.start_recording(
                path, format, fps, speed, idle_time_limit, zoom
            )
        )

    async def stop_recording(self) -> str:
        return await self._await(self._native.stop_recording())

    async def wait_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        whitespace: str = "exact",
        after: Optional[TextAnchor] = None,
        before: Optional[TextAnchor] = None,
        occurrence: TextOccurrence = "any",
        not_: bool = False,
        timeout: Optional[int] = None,
    ) -> Locator:
        locator = self.get_by_text(
            text,
            regex=regex,
            full=full,
            whitespace=whitespace,
            after=after,
            before=before,
            occurrence=occurrence,
        )
        return await locator._wait(
            state="hidden" if not_ else "visible",
            timeout=timeout,
            op_name="wait_text",
        )

    async def wait_title(
        self,
        text: str,
        *,
        regex: bool = False,
        not_: bool = False,
        timeout: Optional[int] = None,
    ) -> None:
        await self._guarded(
            "wait_title",
            self._native.wait_title(
                text, regex, not_, self._timeout("text", timeout)
            ),
        )

    async def wait_idle(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_idle",
            self._native.wait_idle(self._timeout("idle", timeout)),
        )

    async def wait_command(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_command",
            self._native.wait_command(self._timeout("command", timeout)),
        )

    async def wait_exit(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_exit",
            self._native.wait_exit(self._timeout("exit", timeout)),
        )

    async def wait_ready(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_ready",
            self._native.wait_ready(self._timeout("ready", timeout)),
        )

    async def wait_bell(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_bell",
            self._native.wait_bell(self._timeout("text", timeout)),
        )

    async def expect_title(
        self,
        text: str,
        *,
        regex: bool = False,
        not_: bool = False,
        timeout: Optional[int] = None,
    ) -> None:
        await self._guarded(
            "expect_title",
            self._native.expect_title(
                text, regex, not_, self._timeout("text", timeout)
            ),
        )

    async def expect_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        strict: bool = True,
        whitespace: str = "exact",
        after: Optional[TextAnchor] = None,
        before: Optional[TextAnchor] = None,
        occurrence: Optional[TextOccurrence] = None,
        not_: bool = False,
        fg: Optional[str] = None,
        bg: Optional[str] = None,
        style: Optional[TextStyle] = None,
        timeout: Optional[int] = None,
    ) -> None:
        locator = self.get_by_text(
            text,
            regex=regex,
            full=full,
            whitespace=whitespace,
            after=after,
            before=before,
            occurrence=(
                occurrence
                if occurrence is not None
                else ("unique" if strict else "first")
            ),
        )
        style_value = asdict(style or TextStyle())
        if style_value["foreground"] is None:
            style_value["foreground"] = fg
        if style_value["background"] is None:
            style_value["background"] = bg
        await locator._expect(
            not_=not_,
            style=TextStyle(**style_value),
            timeout=timeout,
            op_name="expect_text",
        )

    async def expect_exit_code(
        self, code: int, *, timeout: Optional[int] = None
    ) -> None:
        await self._guarded(
            "expect_exit_code",
            self._native.expect_exit_code(
                code, self._timeout("command", timeout)
            ),
        )

    async def expect_output(self, text: str, *, regex: bool = False) -> None:
        await self._guarded(
            "expect_output", self._native.expect_output(text, regex)
        )

    async def expect_bell_count(
        self, count: int, *, timeout: Optional[int] = None
    ) -> None:
        await self._guarded(
            "expect_bell_count",
            self._native.expect_bell_count(
                count, self._timeout("text", timeout)
            ),
        )

    async def expect_snapshot(
        self,
        name: str,
        *,
        update: bool = False,
        include_colors: bool = False,
        include_title: bool = False,
    ) -> str:
        return await self._guarded(
            "expect_snapshot",
            self._native.snapshot(
                name, update, include_colors, include_title, os.getcwd()
            ),
        )

    async def __aenter__(self) -> "TuiTest":
        return self

    async def __aexit__(self, *exc: Any) -> None:
        await self.close_quiet()


async def sessions() -> List[str]:
    return await _await_native(native.sessions())


async def close_all() -> None:
    await _await_native(native.close_all())


async def get_recording(session: Optional[str] = None) -> str:
    name = cfg.resolve_session(session)
    try:
        return await _await_native(native.recording(name))
    except NoSessionError as error:
        raise NoSessionError(f"no recording for session '{name}'") from error


async def _panic_probe() -> None:
    await _await_native(native.panic_probe())
