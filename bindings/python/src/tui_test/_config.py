from __future__ import annotations

import collections.abc
import dataclasses
import os
import sys
from typing import Dict, Mapping, Optional

VERSION = "0.1.0-beta.1"

DEFAULT_COLS = 80
DEFAULT_ROWS = 30

IS_WINDOWS = sys.platform == "win32"
IS_MACOS = sys.platform == "darwin"


def resolve_session(session: Optional[str]) -> str:
    return session or os.environ.get("TUI_TEST_SESSION") or "default"


_TIMEOUT_CLASSES = ("text", "idle", "command", "exit", "ready")


def resolve_timeout(
    class_name: str,
    *,
    call: Optional[int] = None,
    timeouts: Optional[Mapping[str, Optional[int]]] = None,
) -> Optional[int]:
    if call is not None:
        return call
    if timeouts is not None:
        return timeouts.get(class_name)
    return None


def normalize_timeouts(timeouts: object) -> Optional[Dict[str, Optional[int]]]:
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
    normalized = normalize_timeouts(timeouts)
    if not normalized:
        return None
    payload = {
        class_name: value
        for class_name, value in normalized.items()
        if value is not None
    }
    return payload or None
