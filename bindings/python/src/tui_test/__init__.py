from __future__ import annotations

from ._config import VERSION as __version__
from ._ephemeral import unique_session
from .client import Locator, TuiTest, close_all, get_recording, sessions
from .diagnostics import (
    FailureArtifactRef,
    FailureArtifactStatus,
    FailureDetails,
    FailureReason,
)
from .errors import (
    ExpectationError,
    InternalError,
    NoSessionError,
    TuiTestError,
    TerminalArtifact,
    UsageError,
)
from .types import (
    AutomaticRecording,
    AutomaticRecordingMode,
    Backend,
    BellEvent,
    Cell,
    Colors,
    LocatorDirection,
    MouseButton,
    Profile,
    RecordingFormat,
    State,
    TextMatch,
    TextPosition,
    TextSpan,
    TextStyle,
    Timeouts,
)

__all__ = [
    "TuiTest",
    "Locator",
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
    "FailureDetails",
    "FailureReason",
    "FailureArtifactRef",
    "FailureArtifactStatus",
    "AutomaticRecording",
    "AutomaticRecordingMode",
    "BellEvent",
    "Cell",
    "Backend",
    "Colors",
    "MouseButton",
    "LocatorDirection",
    "Profile",
    "RecordingFormat",
    "State",
    "TextMatch",
    "TextPosition",
    "TextSpan",
    "TextStyle",
    "Timeouts",
    "__version__",
]
