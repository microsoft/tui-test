from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class TerminalArtifact:
    text: Optional[str] = None
    screenshot: Optional[str] = None


class ShellUseError(Exception):
    kind: str = "internal"
    exit_code: int = 5

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message
        self.terminal: Optional[TerminalArtifact] = None


class ExpectationError(ShellUseError):
    kind = "assertion"
    exit_code = 1


class UsageError(ShellUseError):
    kind = "usage"
    exit_code = 2


class NoSessionError(ShellUseError):
    kind = "no_session"
    exit_code = 3


class InternalError(ShellUseError):
    kind = "internal"
    exit_code = 5


_BY_KIND = {
    "assertion": ExpectationError,
    "usage": UsageError,
    "no_session": NoSessionError,
    "internal": InternalError,
}


def make_error(kind: Optional[str], message: str) -> ShellUseError:
    return _BY_KIND.get(kind or "", InternalError)(message)
