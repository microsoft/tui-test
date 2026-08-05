from __future__ import annotations

import atexit
import os
import time
from typing import (
    Any,
    Awaitable,
    Callable,
    Dict,
    Iterable,
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
    UsageError,
)
from .types import Cell, State, Timeouts

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


def _extract_terminal_text(message: Optional[str]) -> Optional[str]:
    if not message:
        return None
    index = message.find(_TERMINAL_MARKER)
    if index == -1:
        return None
    return message[index + len(_TERMINAL_MARKER):].rstrip("\n") or None


class _Mouse:
    def __init__(self, client: "ShellUse") -> None:
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


class ShellUse:
    def __init__(
        self,
        session: Optional[str] = None,
        *,
        timeouts: Optional[Timeouts] = None,
        artifacts: Optional[Dict[str, Any]] = None,
    ) -> None:
        self._session = cfg.resolve_session(session)
        self._native = native.NativeSession(self._session)
        self._timeouts = cfg.normalize_timeouts(timeouts)
        self._artifacts = artifacts
        self._artifact_counter = 0
        self.mouse = _Mouse(self)

    @classmethod
    def ephemeral(cls, prefix: Optional[str] = None, **kwargs: Any) -> "ShellUse":
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
        cols: int = cfg.DEFAULT_COLS,
        rows: int = cfg.DEFAULT_ROWS,
        cwd: Optional[str] = None,
        env: EnvLike = None,
        wait_ready: Optional[bool] = None,
        timeouts: Optional[Timeouts] = None,
        retries: int = 0,
    ) -> Dict[str, Any]:
        env_values = _env_pairs(env)
        timeout_values = _session_timeout_values(timeouts)
        return await self._spawn(
            lambda: self._native.open(
                shell,
                cols,
                rows,
                cwd,
                env_values,
                wait_ready,
                *timeout_values,
            ),
            retries,
        )

    async def run(
        self,
        program: str,
        *args: str,
        cols: int = cfg.DEFAULT_COLS,
        rows: int = cfg.DEFAULT_ROWS,
        cwd: Optional[str] = None,
        env: EnvLike = None,
        wait_ready: Optional[bool] = None,
        timeouts: Optional[Timeouts] = None,
        retries: int = 0,
    ) -> Dict[str, Any]:
        env_values = _env_pairs(env)
        timeout_values = _session_timeout_values(timeouts)
        return await self._spawn(
            lambda: self._native.run(
                program,
                list(args),
                cols,
                rows,
                cwd,
                env_values,
                wait_ready,
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
        await self._await(self._native.press(list(keys)))

    async def keys(self, combo: str) -> None:
        await self._await(self._native.keys(combo))

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

    async def get_cursor(self) -> Dict[str, int]:
        return await self._await(self._native.get_cursor())

    async def get_size(self) -> Dict[str, int]:
        return await self._await(self._native.get_size())

    async def get_bell_count(self) -> int:
        return await self._await(self._native.get_bell_count())

    async def screenshot(
        self, path: Optional[str] = None, *, full: bool = False
    ) -> str:
        return await self._await(self._native.screenshot(path, full))

    async def wait_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        not_: bool = False,
        timeout: Optional[int] = None,
    ) -> None:
        await self._guarded(
            "wait_text",
            self._native.wait_text(
                text, regex, full, not_, self._timeout("text", timeout)
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

    async def expect_text(
        self,
        text: str,
        *,
        regex: bool = False,
        full: bool = False,
        strict: bool = True,
        not_: bool = False,
        fg: Optional[str] = None,
        bg: Optional[str] = None,
        timeout: Optional[int] = None,
    ) -> None:
        await self._guarded(
            "expect_text",
            self._native.expect_text(
                text,
                regex,
                full,
                strict,
                not_,
                fg,
                bg,
                self._timeout("text", timeout),
            ),
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
    ) -> str:
        return await self._guarded(
            "expect_snapshot",
            self._native.snapshot(
                name, update, include_colors, os.getcwd()
            ),
        )

    async def __aenter__(self) -> "ShellUse":
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
