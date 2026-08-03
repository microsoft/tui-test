from __future__ import annotations

import collections.abc
import dataclasses
import os
import sys
from pathlib import Path
from typing import Dict, Mapping, Optional

VERSION = "0.0.1-beta.5"

DEFAULT_COLS = 80
DEFAULT_ROWS = 30

IS_WINDOWS = sys.platform == "win32"
IS_MACOS = sys.platform == "darwin"


def resolve_session(session: Optional[str]) -> str:
    return session or os.environ.get("SHELL_USE_SESSION") or "default"


def resolve_binary(binary: Optional[str]) -> str:
    return binary or os.environ.get("SHELL_USE_BIN") or "shell-use"


def resolve_home(home: Optional[str]) -> Optional[str]:
    return home or os.environ.get("SHELL_USE_HOME") or None


def home_dir(home: Optional[str]) -> Path:
    return Path(home) if home else Path.home() / ".shell-use"


def socket_path(session: str, home: Optional[str]) -> str:
    if IS_WINDOWS:
        return rf"\\.\pipe\shell-use-{session}.sock"
    return str(home_dir(home) / f"{session}.sock")


def _cache_dir() -> Path:
    if IS_WINDOWS:
        base = os.environ.get("LOCALAPPDATA")
        return Path(base) if base else Path.home() / "AppData" / "Local"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Caches"
    xdg = os.environ.get("XDG_CACHE_HOME")
    return Path(xdg) if xdg else Path.home() / ".cache"


def recording_dir(home: Optional[str]) -> Path:
    if home:
        return Path(home) / "recordings"
    return _cache_dir() / "shell-use"


def recording_path(session: str, home: Optional[str]) -> Path:
    return recording_dir(home) / f"{session}.cast"


_TIMEOUT_CLASSES = ("text", "idle", "command", "exit", "ready")


def resolve_timeout(
    class_name: str,
    *,
    call: Optional[int] = None,
    timeouts: Optional[Mapping[str, Optional[int]]] = None,
) -> Optional[int]:
    """Resolve a client-side timeout; ``None`` means omit it so the daemon applies its own default."""
    if call is not None:
        return call
    if timeouts is not None:
        return timeouts.get(class_name)
    return None


def normalize_timeouts(timeouts: object) -> Optional[Dict[str, Optional[int]]]:
    """Coerce timeouts to a dict; unrecognised keys raise instead of being ignored by the daemon."""
    if timeouts is None:
        return None
    if dataclasses.is_dataclass(timeouts) and not isinstance(timeouts, type):
        return dataclasses.asdict(timeouts)
    if isinstance(timeouts, collections.abc.Mapping):
        normalized = dict(timeouts)
        unknown = sorted(set(normalized) - set(_TIMEOUT_CLASSES))
        if unknown:
            raise ValueError(
                "unknown timeout class {}; expected one of {}".format(
                    ", ".join(repr(name) for name in unknown),
                    ", ".join(_TIMEOUT_CLASSES),
                )
            )
        return normalized
    raise TypeError("timeouts must be a Timeouts, a mapping, or None")


def session_timeouts_payload(timeouts: object) -> Optional[Dict[str, int]]:
    """Build the session timeout payload, omitting unset fields so the daemon applies its own default."""
    normalized = normalize_timeouts(timeouts)
    if not normalized:
        return None
    payload = {
        class_name: value
        for class_name, value in normalized.items()
        if value is not None
    }
    return payload or None

