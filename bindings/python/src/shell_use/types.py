from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Optional, Union

Color = Union[str, int]


@dataclass
class Timeouts:
    """Client-level default timeouts in milliseconds; unset fields are omitted so the daemon supplies them."""

    text: Optional[int] = None
    idle: Optional[int] = None
    command: Optional[int] = None
    exit: Optional[int] = None
    ready: Optional[int] = None


@dataclass
class Cell:
    x: int
    y: int
    char: str
    fg: Color
    bg: Color
    bold: bool
    italic: bool
    underline: bool
    inverse: bool


@dataclass
class State:
    cols: int
    rows: int
    cursor: Dict[str, int]
    cwd: Optional[str]
    last_command: Optional[str]
    last_exit: Optional[int]
    exited: Optional[int]
    ready: bool
    text: str
    session_shell: Optional[str]

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "State":
        return cls(
            cols=d.get("cols", 0),
            rows=d.get("rows", 0),
            cursor=d.get("cursor", {"x": 0, "y": 0}),
            cwd=d.get("cwd"),
            last_command=d.get("last_command"),
            last_exit=d.get("last_exit"),
            exited=d.get("exited"),
            ready=d.get("ready", False),
            text=d.get("text", ""),
            session_shell=d.get("session_shell"),
        )
