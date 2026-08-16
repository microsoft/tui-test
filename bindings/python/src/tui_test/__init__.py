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
from .types import (
    Backend,
    BellEvent,
    Cell,
    Colors,
    Profile,
    RecordingFormat,
    State,
    TextAnchor,
    TextMatch,
    TextOccurrence,
    TextPosition,
    TextSpan,
    TextStyle,
    Timeouts,
)

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
    "BellEvent",
    "Cell",
    "Backend",
    "Colors",
    "Profile",
    "RecordingFormat",
    "State",
    "TextAnchor",
    "TextMatch",
    "TextOccurrence",
    "TextPosition",
    "TextSpan",
    "TextStyle",
    "Timeouts",
    "__version__",
]
