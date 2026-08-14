from __future__ import annotations

from ._config import VERSION as __version__
from ._ephemeral import unique_session
from .client import TuiTest, close_all, get_recording, sessions
from .errors import (
    ExpectationError,
    InternalError,
    NoSessionError,
    TuiTestError,
    TerminalArtifact,
    UsageError,
)
from .types import Cell, Colors, Profile, State, Timeouts

__all__ = [
    "TuiTest",
    "sessions",
    "close_all",
    "get_recording",
    "unique_session",
    "TuiTestError",
    "ExpectationError",
    "UsageError",
    "NoSessionError",
    "InternalError",
    "TerminalArtifact",
    "Cell",
    "Colors",
    "Profile",
    "State",
    "Timeouts",
    "__version__",
]
