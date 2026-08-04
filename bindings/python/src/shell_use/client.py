from __future__ import annotations

import asyncio
import atexit
import os
import time
from typing import Any, Callable, Dict, List, Optional, TypeVar

from . import _config as cfg
from . import _ephemeral as ephemeral
from . import _native as native
from ._protocol import EnvLike, env_pairs, unwrap
from .errors import ExpectationError, NoSessionError, TerminalArtifact
from .types import Cell, State, Timeouts

_TERMINAL_MARKER = "Terminal content:\n"

_T = TypeVar("_T")


async def _to_thread(func: Callable[..., _T], *args: Any) -> _T:
    # asyncio.to_thread requires Python 3.9.
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(None, func, *args)


def _atexit_close_all() -> None:
    try:
        native.close_all()
    except Exception:
        pass


atexit.register(_atexit_close_all)


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
        await self._c.send(
            {
                "kind": "mouse",
                "action": {
                    "op": "click",
                    "x": x,
                    "y": y,
                    "on_text": on_text,
                    "button": button,
                    "clicks": clicks,
                },
            }
        )

    async def move(self, x: int, y: int) -> None:
        await self._c.send({"kind": "mouse", "action": {"op": "move", "x": x, "y": y}})

    async def down(self, x: int, y: int, *, button: int = 0) -> None:
        await self._c.send(
            {"kind": "mouse", "action": {"op": "down", "x": x, "y": y, "button": button}}
        )

    async def up(self, x: int, y: int, *, button: int = 0) -> None:
        await self._c.send(
            {"kind": "mouse", "action": {"op": "up", "x": x, "y": y, "button": button}}
        )

    async def drag(
        self, x1: int, y1: int, x2: int, y2: int, *, button: int = 0
    ) -> None:
        await self._c.send(
            {
                "kind": "mouse",
                "action": {
                    "op": "drag",
                    "x1": x1,
                    "y1": y1,
                    "x2": x2,
                    "y2": y2,
                    "button": button,
                },
            }
        )

    async def scroll(self, direction: str, *, amount: int = 3) -> None:
        await self._c.send(
            {
                "kind": "mouse",
                "action": {"op": "scroll", "direction": direction, "amount": amount},
            }
        )


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

    def _with_timeout(
        self, payload: Dict[str, Any], class_name: str, call: Optional[int]
    ) -> Dict[str, Any]:
        value = cfg.resolve_timeout(class_name, call=call, timeouts=self._timeouts)
        if value is not None:
            payload["timeout_ms"] = value
        return payload

    async def send(self, payload: Dict[str, Any]) -> Any:
        resp = await _to_thread(self._native.request, payload)
        return unwrap(resp)

    async def _guarded(self, op_name: str, payload: Dict[str, Any]) -> Any:
        try:
            return await self.send(payload)
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
        n = self._artifact_counter
        timestamp = time.strftime("%Y%m%d-%H%M%S")
        filename = "{}-{}-{}.svg".format(self._session, timestamp, n)
        path = os.path.join(directory, filename)
        await self.screenshot(path)
        return path

    async def _spawn(self, payload: Dict[str, Any], retries: int) -> Dict[str, Any]:
        attempts = retries + 1 if retries > 0 else 1
        for attempt in range(attempts):
            try:
                return await self.send(payload)
            except Exception:
                if attempt + 1 < attempts:
                    await self.close_quiet()
                else:
                    raise

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
        payload = {
            "kind": "open",
            "shell": shell,
            "program": None,
            "cols": cols,
            "rows": rows,
            "cwd": cwd,
            "env": env_pairs(env),
        }  # type: Dict[str, Any]
        if wait_ready is not None:
            payload["wait_ready"] = wait_ready
        session_timeouts = cfg.session_timeouts_payload(timeouts)
        if session_timeouts is not None:
            payload["timeouts"] = session_timeouts
        return await self._spawn(payload, retries)

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
        payload = {
            "kind": "open",
            "shell": None,
            "program": [program, *args],
            "cols": cols,
            "rows": rows,
            "cwd": cwd,
            "env": env_pairs(env),
        }  # type: Dict[str, Any]
        if wait_ready is not None:
            payload["wait_ready"] = wait_ready
        session_timeouts = cfg.session_timeouts_payload(timeouts)
        if session_timeouts is not None:
            payload["timeouts"] = session_timeouts
        return await self._spawn(payload, retries)

    async def close(self) -> None:
        await self.send({"kind": "close"})

    async def close_quiet(self) -> None:
        try:
            await self.close()
        except Exception:
            pass

    async def type(self, text: str) -> None:
        await self.send({"kind": "write", "data": text})

    async def write(self, data: str) -> None:
        await self.send({"kind": "write", "data": data})

    async def submit(self, text: Optional[str] = None) -> None:
        await self.send({"kind": "submit", "data": text})

    async def press(self, *keys: str) -> None:
        await self.send({"kind": "press", "keys": list(keys)})

    async def keys(self, combo: str) -> None:
        await self.send({"kind": "press", "keys": [combo]})

    async def resize(self, cols: int, rows: int) -> None:
        await self.send({"kind": "resize", "cols": cols, "rows": rows})

    async def signal(self, name: str) -> None:
        await self.send({"kind": "signal", "name": name})

    async def kill(self) -> None:
        await self.send({"kind": "signal", "name": "KILL"})

    async def state(self) -> State:
        return State.from_dict(await self.send({"kind": "state"}))

    async def text(self, *, full: bool = False) -> str:
        return (await self.send({"kind": "text", "full": full}))["text"]

    async def cells(self, x: int, y: int, w: int = 1, h: int = 1) -> List[Cell]:
        data = await self.send({"kind": "cells", "x": x, "y": y, "w": w, "h": h})
        return [Cell(**c) for c in data["cells"]]

    async def get(self, field: str) -> Any:
        return (await self.send({"kind": "get", "field": field}))["value"]

    async def get_command(self) -> Optional[str]:
        return await self.get("command")

    async def get_output(self) -> Optional[str]:
        return await self.get("output")

    async def get_exit_code(self) -> Optional[int]:
        return await self.get("exit-code")

    async def get_cwd(self) -> Optional[str]:
        return await self.get("cwd")

    async def get_cursor(self) -> Dict[str, int]:
        return await self.get("cursor")

    async def get_size(self) -> Dict[str, int]:
        return await self.get("size")

    async def screenshot(self, path: Optional[str] = None, *, full: bool = False) -> str:
        data = await self.send({"kind": "screenshot", "full": full, "path": path})
        return data.get("path") or data.get("text")

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
            self._with_timeout(
                {
                    "kind": "wait_text",
                    "text": text,
                    "regex": regex,
                    "full": full,
                    "not": not_,
                },
                "text",
                timeout,
            ),
        )

    async def wait_idle(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_idle",
            self._with_timeout({"kind": "wait_idle"}, "idle", timeout),
        )

    async def wait_command(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_command",
            self._with_timeout({"kind": "wait_command"}, "command", timeout),
        )

    async def wait_exit(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_exit",
            self._with_timeout({"kind": "wait_exit"}, "exit", timeout),
        )

    async def wait_ready(self, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "wait_ready",
            self._with_timeout({"kind": "wait_ready"}, "ready", timeout),
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
            self._with_timeout(
                {
                    "kind": "expect_text",
                    "text": text,
                    "regex": regex,
                    "full": full,
                    "strict": strict,
                    "not": not_,
                    "fg": fg,
                    "bg": bg,
                },
                "text",
                timeout,
            ),
        )

    async def expect_exit_code(self, code: int, *, timeout: Optional[int] = None) -> None:
        await self._guarded(
            "expect_exit_code",
            self._with_timeout(
                {"kind": "expect_exit_code", "code": code}, "command", timeout
            ),
        )

    async def expect_output(self, text: str, *, regex: bool = False) -> None:
        await self._guarded(
            "expect_output", {"kind": "expect_output", "text": text, "regex": regex}
        )

    async def expect_snapshot(
        self, name: str, *, update: bool = False, include_colors: bool = False
    ) -> str:
        return (
            await self._guarded(
                "expect_snapshot",
                {
                    "kind": "snapshot",
                    "name": name,
                    "update": update,
                    "include_colors": include_colors,
                    "cwd": os.getcwd(),
                },
            )
        )["status"]

    async def __aenter__(self) -> "ShellUse":
        return self

    async def __aexit__(self, *exc: Any) -> None:
        await self.close_quiet()


async def sessions() -> List[str]:
    return await _to_thread(native.sessions)


async def close_all() -> None:
    await _to_thread(native.close_all)


async def get_recording(session: Optional[str] = None) -> str:
    name = cfg.resolve_session(session)
    try:
        return await _to_thread(native.recording, name)
    except FileNotFoundError:
        raise NoSessionError(f"no recording for session '{name}'")
