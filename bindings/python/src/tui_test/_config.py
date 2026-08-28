from __future__ import annotations

import collections.abc
import dataclasses
import os
import sys
from typing import Any, Dict, Mapping, Optional

VERSION = "0.1.0-beta.2"

DEFAULT_COLS = 80
DEFAULT_ROWS = 30

IS_WINDOWS = sys.platform == "win32"
IS_MACOS = sys.platform == "darwin"


def resolve_session(session: Optional[str]) -> str:
    return session or os.environ.get("TUI_TEST_SESSION") or "default"


_TIMEOUT_CLASSES = ("text", "idle", "command", "exit", "ready")
_BACKENDS = ("alacritty", "ghostty", "rio", "xtermjs")
_RECORDING_MODES = ("disabled", "on-failure", "always")
_PROFILE_FIELDS = frozenset(("scrollback", "colors"))
_COLOR_FIELDS = frozenset(
    (
        "foreground",
        "background",
        "cursor",
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
    )
)


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


def normalize_backend(backend: object) -> Optional[str]:
    if backend is None:
        return None
    if not isinstance(backend, str):
        raise TypeError("backend must be a string or None")
    normalized = backend.strip().lower()
    if normalized not in _BACKENDS:
        raise ValueError(
            "unknown backend {!r}; expected one of {}".format(
                backend, ", ".join(_BACKENDS)
            )
        )
    return normalized


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


def _object_mapping(value: object, name: str) -> Dict[str, Any]:
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return dataclasses.asdict(value)
    if isinstance(value, collections.abc.Mapping):
        return dict(value)
    raise TypeError("{} must be a dataclass or mapping".format(name))


def normalize_recording(recording: object) -> Optional[Dict[str, Any]]:
    if recording is None:
        return None
    raw = _object_mapping(recording, "recording")
    unknown = sorted(set(raw) - {"mode", "directory"})
    if unknown:
        raise ValueError(
            "unknown recording field {}".format(
                ", ".join(repr(name) for name in unknown)
            )
        )
    mode = raw.get("mode")
    if mode is not None and mode not in _RECORDING_MODES:
        raise ValueError(
            "unknown recording mode {!r}; expected one of {}".format(
                mode, ", ".join(_RECORDING_MODES)
            )
        )
    directory = raw.get("directory")
    if directory is not None and not isinstance(directory, str):
        raise TypeError("recording.directory must be a string")
    return raw


def normalize_profile(profile: object) -> Optional[Dict[str, Any]]:
    if profile is None:
        return None
    raw = _object_mapping(profile, "profile")
    unknown = sorted(set(raw) - _PROFILE_FIELDS)
    if unknown:
        raise ValueError(
            "unknown profile field {}".format(
                ", ".join(repr(name) for name in unknown)
            )
        )

    normalized = {}  # type: Dict[str, Any]
    if raw.get("scrollback") is not None:
        normalized["scrollback"] = raw["scrollback"]

    raw_colors = raw.get("colors")
    if raw_colors is not None:
        colors = _object_mapping(raw_colors, "profile.colors")
        unknown = sorted(set(colors) - _COLOR_FIELDS)
        if unknown:
            raise ValueError(
                "unknown profile color {}".format(
                    ", ".join(repr(name) for name in unknown)
                )
            )
        normalized_colors = {}
        for name, value in colors.items():
            if value is None:
                continue
            if not isinstance(value, str):
                raise TypeError("profile.colors.{} must be a string".format(name))
            normalized_colors[name] = value
        if normalized_colors:
            normalized["colors"] = normalized_colors
    return normalized
