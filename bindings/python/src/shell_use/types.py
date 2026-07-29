from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Literal, Optional, Union

Color = Union[str, int]
UnderlineStyle = Literal["single", "double", "curly", "dotted", "dashed"]


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
    underline: bool
    underline_style: Optional[UnderlineStyle]
    #: ``None`` means the underline follows the text color.
    underline_color: Optional[Color]


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
