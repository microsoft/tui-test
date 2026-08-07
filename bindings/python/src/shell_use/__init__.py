from __future__ import annotations

from ._config import VERSION as __version__
from ._ephemeral import unique_session
from .client import ShellUse, close_all, get_recording, sessions
from .errors import (
    ExpectationError,
    InternalError,
    NoSessionError,
    ShellUseError,
    TerminalArtifact,
    UsageError,
)
from .types import Cell, RecordingFormat, State, Timeouts

__all__ = [
    "ShellUse",
    "sessions",
    "close_all",
    "get_recording",
    "unique_session",
    "ShellUseError",
    "ExpectationError",
    "UsageError",
    "NoSessionError",
    "InternalError",
    "TerminalArtifact",
    "Cell",
    "RecordingFormat",
    "State",
    "Timeouts",
    "__version__",
]
