from __future__ import annotations

import asyncio
import atexit
import re
import threading
from contextlib import asynccontextmanager
from dataclasses import dataclass, replace
from typing import (
    Any,
    AsyncIterator,
    Dict,
    Iterable,
    Optional,
    Sequence,
    Set,
)

from ._config import IS_MACOS, IS_WINDOWS
from ._ephemeral import unique_session
from .client import TuiTest
from .types import AutomaticRecording, Backend, Profile, Timeouts

__all__ = [
    "TerminalOptions",
    "DEFAULT_SHELL",
    "create_terminal",
    "terminal",
    "set_terminal_defaults",
    "reset_terminal_defaults",
    "track_terminal",
    "untrack_terminal",
    "tracked_count",
    "close_all_tracked",
    "terminal_snapshot",
]

DEFAULT_SHELL = "powershell" if IS_WINDOWS else "zsh" if IS_MACOS else "bash"


@dataclass
class TerminalOptions:
    backend: Optional[Backend] = None
    shell: Optional[str] = None
    program: Optional[Sequence[str]] = None
    cols: Optional[int] = None
    rows: Optional[int] = None
    cwd: Optional[str] = None
    env: Optional[Any] = None
    session: Optional[str] = None
    prefix: Optional[str] = None
    retries: Optional[int] = None
    wait_ready: Optional[bool] = None
    timeouts: Optional[Timeouts] = None
    profile: Optional[Profile] = None
    artifacts: Optional[Dict[str, Any]] = None
    recording: Optional[AutomaticRecording] = None


_DEFAULTABLE = frozenset(TerminalOptions.__dataclass_fields__)
_defaults = TerminalOptions()
_defaults_lock = threading.Lock()


def set_terminal_defaults(**values: Any) -> None:
    unknown = sorted(set(values) - _DEFAULTABLE)
    if unknown:
        raise TypeError(
            "unknown terminal option {}".format(", ".join(repr(k) for k in unknown))
        )
    global _defaults
    with _defaults_lock:
        _defaults = replace(_defaults, **values)


def get_terminal_defaults() -> TerminalOptions:
    return _defaults


def reset_terminal_defaults() -> None:
    global _defaults
    with _defaults_lock:
        _defaults = TerminalOptions()


_tracked = set()  # type: Set[TuiTest]
_tracked_lock = threading.Lock()
_safety_net_installed = False


def _install_safety_net() -> None:
    global _safety_net_installed
    if _safety_net_installed:
        return
    _safety_net_installed = True
    atexit.register(_close_all_tracked_blocking)


def _close_all_tracked_blocking() -> None:
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        pass
    else:  # pragma: no cover - atexit never runs inside a live loop
        return
    with _tracked_lock:
        pending = list(_tracked)
        _tracked.clear()
    if not pending:
        return
    try:
        asyncio.run(_close_quietly(pending))
    except Exception:  # pragma: no cover - best effort at shutdown
        pass


async def _close_quietly(terminals: Iterable[TuiTest]) -> None:
    await asyncio.gather(*(t.close_quiet() for t in terminals))


def track_terminal(term: TuiTest) -> None:
    with _tracked_lock:
        _tracked.add(term)
    _install_safety_net()


def untrack_terminal(term: TuiTest) -> None:
    with _tracked_lock:
        _tracked.discard(term)


def tracked_count() -> int:
    with _tracked_lock:
        return len(_tracked)


async def close_all_tracked() -> None:
    with _tracked_lock:
        pending = list(_tracked)
        _tracked.clear()
    await _close_quietly(pending)



def _client_kwargs(opts: TerminalOptions) -> Dict[str, Any]:
    kwargs = {}  # type: Dict[str, Any]
    if opts.backend is not None:
        kwargs["backend"] = opts.backend
    if opts.timeouts is not None:
        kwargs["timeouts"] = opts.timeouts
    if opts.profile is not None:
        kwargs["profile"] = opts.profile
    if opts.artifacts is not None:
        kwargs["artifacts"] = opts.artifacts
    if opts.recording is not None:
        kwargs["recording"] = opts.recording
    return kwargs


def _spawn_kwargs(opts: TerminalOptions) -> Dict[str, Any]:
    kwargs = {"retries": 2 if opts.retries is None else opts.retries}  # type: Dict[str, Any]
    for name in ("cols", "rows", "cwd", "env", "wait_ready"):
        value = getattr(opts, name)
        if value is not None:
            kwargs[name] = value
    return kwargs


async def create_terminal(**options: Any) -> TuiTest:
    per_call = TerminalOptions(**options)
    merged = dict(_defaults.__dict__)
    for key, value in per_call.__dict__.items():
        if value is not None:
            merged[key] = value
    opts = TerminalOptions(**merged)
    session = opts.session or unique_session(opts.prefix)
    term = TuiTest(session, **_client_kwargs(opts))
    track_terminal(term)
    spawn = _spawn_kwargs(opts)
    try:
        if opts.program:
            program = list(opts.program)
            # ``run`` takes the argv tail as varargs, not a list.
            await term.run(program[0], *program[1:], **spawn)
        else:
            await term.open(shell=opts.shell, **spawn)
    except BaseException:
        await term.close_quiet()
        untrack_terminal(term)
        raise
    return term


@asynccontextmanager
async def terminal(**options: Any) -> AsyncIterator[TuiTest]:
    term = await create_terminal(**options)
    try:
        yield term
    finally:
        await term.close_quiet()
        untrack_terminal(term)


_TRAILING_WS = re.compile(r"\s+$")


def terminal_snapshot(text: str) -> str:
    lines = [_TRAILING_WS.sub("", line) for line in text.split("\n")]
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines)
