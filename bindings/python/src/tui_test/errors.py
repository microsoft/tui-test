from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping, Optional, Tuple

from .diagnostics import FailureArtifactRef, FailureDetails


@dataclass
class TerminalArtifact:
    text: Optional[str] = None
    screenshot: Optional[str] = None
    errors: Tuple[str, ...] = ()


class TuiTestError(Exception):
    kind: str = "internal"
    exit_code: int = 5

    def __init__(
        self,
        message: str,
        *,
        details: Optional[FailureDetails] = None,
        artifact: Optional[FailureArtifactRef] = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.details = details
        self.artifact = artifact
        self.terminal: Optional[TerminalArtifact] = None


class ExpectationError(TuiTestError):
    kind = "assertion"
    exit_code = 1


class UsageError(TuiTestError):
    kind = "usage"
    exit_code = 2


class NoSessionError(TuiTestError):
    kind = "no_session"
    exit_code = 3


class InternalError(TuiTestError):
    kind = "internal"
    exit_code = 5


_BY_KIND = {
    "assertion": ExpectationError,
    "usage": UsageError,
    "no_session": NoSessionError,
    "internal": InternalError,
}


def make_error(
    kind: Optional[str],
    message: str,
    *,
    details: Optional[Mapping[str, object]] = None,
    artifact: Optional[Mapping[str, object]] = None,
) -> TuiTestError:
    return _BY_KIND.get(kind or "", InternalError)(
        message,
        details=FailureDetails.from_dict(details) if details is not None else None,
        artifact=(
            FailureArtifactRef.from_dict(artifact)
            if artifact is not None
            else None
        ),
    )
