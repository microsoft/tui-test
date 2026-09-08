from __future__ import annotations

import collections.abc
import dataclasses
import os
import sys
from typing import Any, Dict, Mapping, Optional

from .types import MonitoringMetadata, _UNSET

VERSION = "0.1.0-beta.3"

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


def resolve_monitoring(monitoring: object = None) -> Dict[str, Any]:
    raw = {} if monitoring is None else _object_mapping(monitoring, "monitoring")
    raw = {key: value for key, value in raw.items() if value is not _UNSET}
    fields = {
        "enabled", "wait_at_end", "first_attach_timeout",
        "hold_while_attached", "label", "metadata",
    }
    unknown = set(raw) - fields
    if unknown:
        raise ValueError("unknown monitoring field {}".format(
            ", ".join(sorted(repr(key) for key in unknown))
        ))
    wait = raw.get("wait_at_end", os.environ.get("TUI_TEST_WAIT_AT_END", "never"))
    if not isinstance(wait, str) or wait not in ("never", "failure", "always"):
        raise ValueError("monitoring.wait_at_end must be never, failure, or always")
    if "first_attach_timeout" in raw:
        timeout = raw["first_attach_timeout"]
    else:
        env_timeout = os.environ.get("TUI_TEST_FIRST_ATTACH_TIMEOUT")
        if env_timeout is not None:
            env_timeout = env_timeout.strip()
        if env_timeout is None:
            timeout = 30_000
        elif env_timeout.lower() == "infinite":
            timeout = None
        elif env_timeout.isascii() and env_timeout.isdecimal():
            timeout = int(env_timeout)
        else:
            raise ValueError(
                "TUI_TEST_FIRST_ATTACH_TIMEOUT must be a non-negative integer or infinite"
            )
    if timeout is not None and (
        isinstance(timeout, bool) or not isinstance(timeout, int)
        or timeout < 0 or timeout > 2**64 - 1
    ):
        raise TypeError(
            "monitoring.first_attach_timeout must be a non-negative 64-bit integer or None"
        )
    for key in ("enabled", "hold_while_attached"):
        if key in raw and not isinstance(raw[key], bool):
            raise TypeError("monitoring.{} must be a boolean".format(key))
    label = raw.get("label", os.environ.get("TUI_TEST_LABEL"))
    if ("label" in raw or label is not None) and not isinstance(label, str):
        raise TypeError("monitoring.label must be a string")
    metadata_value = raw.get("metadata", {})
    metadata = _object_mapping(metadata_value, "monitoring.metadata")
    # Optional dataclass metadata fields are omitted, not JSON null values.
    if dataclasses.is_dataclass(monitoring):
        original = getattr(monitoring, "metadata", None)
    else:
        original = metadata_value
    if isinstance(original, MonitoringMetadata):
        metadata = {key: value for key, value in metadata.items() if value is not None}
    unknown = set(metadata) - {"test_file", "test_name", "framework", "worker"}
    if unknown:
        raise ValueError("unknown monitoring metadata field {}".format(
            ", ".join(sorted(repr(key) for key in unknown))
        ))
    for key, value in metadata.items():
        if not isinstance(value, str):
            raise TypeError("monitoring.metadata.{} must be a string".format(key))
    enabled = raw.get(
        "enabled",
        os.environ.get("TUI_TEST_MONITORING", "").lower() in ("1", "true")
        or wait != "never",
    )
    return {
        "enabled": enabled,
        "wait_at_end": wait,
        "first_attach_timeout": timeout,
        "hold_while_attached": raw.get("hold_while_attached", True),
        "label": label,
        "metadata": metadata,
    }


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
    if directory is not None and (
        not isinstance(directory, str) or not directory
    ):
        raise TypeError("recording.directory must be a non-empty string")
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
