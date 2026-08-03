from __future__ import annotations

import os
import time
from typing import Any, Dict, List, Optional, Union

from . import _config as cfg
from . import _ephemeral as ephemeral
from . import _transport as transport
from ._protocol import EnvLike, env_pairs, unwrap
from .errors import (
    DaemonError,
    ExpectationError,
    NoSessionError,
    TerminalArtifact,
    VersionMismatchError,
)
from .types import Cell, State, Timeouts

_TERMINAL_MARKER = "Terminal content:\n"


def check_version(daemon_version: Optional[str]) -> None:
    if daemon_version != cfg.VERSION:
        raise VersionMismatchError(
            f"shell-use version mismatch: client {cfg.VERSION}, daemon "
            f"{daemon_version or 'unknown'}. Ensure the shell-use binary matches the "
            "shell-use package version, or stop the daemon (daemon_stop) so it "
            "restarts with the current binary."
        )


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
        binary: Optional[str] = None,
        home: Optional[str] = None,
        isolated: bool = False,
        timeouts: Optional[Timeouts] = None,
        artifacts: Optional[Dict[str, Any]] = None,
    ) -> None:
        self._session = cfg.resolve_session(session)
        self._binary = cfg.resolve_binary(binary)
        self._home_input = home
        self._isolated = isolated
        self._temp_home = None  # type: Optional[str]
        self._resolved_home = None  # type: Optional[str]
        self._home_ready = False
        self._timeouts = cfg.normalize_timeouts(timeouts)
        self._artifacts = artifacts
        self._artifact_counter = 0
        self._version_checked = False
        self._closed = False
        self.mouse = _Mouse(self)

    @classmethod
    def ephemeral(cls, prefix: Optional[str] = None, **kwargs: Any) -> "ShellUse":
        """Return a client bound to a unique session and an isolated home."""
        kwargs["isolated"] = True
        return cls(ephemeral.unique_session(prefix), **kwargs)

    @property
    def session(self) -> str:
        return self._session

    def _ensure_home(self) -> Optional[str]:
        if self._home_ready:
            return self._resolved_home
        if self._isolated:
            home = ephemeral.provision_temp_home()
            self._temp_home = home
        else:
            home = cfg.resolve_home(self._home_input)
        self._resolved_home = home
        self._home_ready = True
        return home

    def _cleanup_temp_home(self) -> None:
        temp = self._temp_home
        if temp is None:
            return
        self._temp_home = None
        if self._resolved_home == temp:
            self._resolved_home = None
            self._home_ready = False
        ephemeral.remove_temp_home(temp)

    def _with_timeout(
        self, payload: Dict[str, Any], class_name: str, call: Optional[int]
    ) -> Dict[str, Any]:
        """Omit ``timeout_ms`` when unset so the daemon applies the session default / env / built-in."""
        value = cfg.resolve_timeout(class_name, call=call, timeouts=self._timeouts)
        if value is not None:
            payload["timeout_ms"] = value
        return payload

    async def send(self, payload: Dict[str, Any]) -> Any:
        home = self._ensure_home()
        await self._check_version(home)
        resp = await transport.request(self._session, home, self._binary, payload)
        return unwrap(resp)

    async def _check_version(self, home: Optional[str]) -> None:
        if self._version_checked:
            return
        resp = await transport.request(
            self._session, home, self._binary, {"kind": "status"}
        )
        data = unwrap(resp)
        check_version(data.get("version") if isinstance(data, dict) else None)
        self._version_checked = True

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
            self._closed = False
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
        if self._closed:
            return
        if not self._home_ready and self._isolated:
            # A private home that was never provisioned has no daemon to close.
            self._closed = True
            return
        self._closed = True
        home = self._ensure_home()
        try:
            if await transport.can_connect(self._session, home):
                resp = await transport.request(
                    self._session, home, self._binary, {"kind": "close"}, autostart=False
                )
                unwrap(resp)
        except (DaemonError, NoSessionError):
            pass
        finally:
            self._cleanup_temp_home()

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


async def sessions(*, home: Optional[str] = None) -> List[str]:
    h = cfg.resolve_home(home)
    directory = cfg.home_dir(h)
    out: List[str] = []
    if directory.is_dir():
        for entry in sorted(directory.iterdir()):
            if entry.suffix == ".pid":
                name = entry.stem
                if await transport.can_connect(name, h):
                    out.append(name)
    return out


async def close_all(*, binary: Optional[str] = None, home: Optional[str] = None) -> None:
    h = cfg.resolve_home(home)
    b = cfg.resolve_binary(binary)
    for name in await sessions(home=h):
        try:
            await transport.request(name, h, b, {"kind": "close"}, autostart=False)
        except Exception:
            pass


async def daemon_status(
    session: Optional[str] = None,
    *,
    binary: Optional[str] = None,
    home: Optional[str] = None,
) -> Dict[str, Any]:
    s = cfg.resolve_session(session)
    h = cfg.resolve_home(home)
    b = cfg.resolve_binary(binary)
    return unwrap(await transport.request(s, h, b, {"kind": "status"}))


async def daemon_stop(
    session: Optional[str] = None,
    *,
    binary: Optional[str] = None,
    home: Optional[str] = None,
) -> None:
    s = cfg.resolve_session(session)
    h = cfg.resolve_home(home)
    b = cfg.resolve_binary(binary)
    if not await transport.can_connect(s, h):
        return
    unwrap(await transport.request(s, h, b, {"kind": "shutdown"}, autostart=False))


async def get_recording(
    session: Optional[str] = None, *, home: Optional[str] = None
) -> str:
    import asyncio

    s = cfg.resolve_session(session)
    h = cfg.resolve_home(home)
    path = cfg.recording_path(s, h)
    loop = asyncio.get_running_loop()
    try:
        data = await loop.run_in_executor(None, path.read_bytes)
    except FileNotFoundError:
        from .errors import NoSessionError

        raise NoSessionError(f"no recording for session '{s}'")
    return data.decode("utf-8", errors="replace")
