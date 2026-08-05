from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Literal, Optional, Union

Color = Union[str, int]
#: ``"none"`` is a value, not an absence: an un-underlined cell reports it.
UnderlineStyle = Literal["none", "single", "double", "curly", "dotted", "dashed"]


@dataclass
class Timeouts:
    text: Optional[int] = None
    idle: Optional[int] = None
    command: Optional[int] = None
    exit: Optional[int] = None
    ready: Optional[int] = None


@dataclass
class Cell:
    x: int
    y: int
    #: The cell's grapheme; ``" "`` when blank, ``""`` for the second column
    #: of a double-width character.
    char: str
    fg: Color
    bg: Color
    bold: bool
    dim: bool
    italic: bool
    inverse: bool
    invisible: bool
    strike: bool
    #: Always ``False`` from the alacritty backend, which cannot report blink.
    blink: bool
    #: Shorthand for ``underline_style != "none"``.
    underline: bool
    underline_style: UnderlineStyle
    #: ``"default"`` means the underline follows the text color. Tracked
    #: independently of ``underline_style``, so a cell that set SGR 58 without
    #: an underline still reports the color it would use.
    underline_color: Color


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
    timeouts: Timeouts
    text: str
    session_shell: Optional[str]
    bell_count: int = 0

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
            bell_count=d.get("bell_count", 0),
            timeouts=Timeouts(**d["timeouts"]),
            text=d.get("text", ""),
            session_shell=d.get("session_shell"),
        )
