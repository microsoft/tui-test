"""Run with PYTHONPATH=bindings/python/src and TUI_TEST_NATIVE_LIBRARY."""

import asyncio
from copy import deepcopy
from pathlib import Path
import sys

from tui_test import (
    BorderStyle, FontFamilies, PaddingSides, RecordingCursorStyle,
    RenderStyle, ShadowStyle, TuiTest, WindowStyle,
)


async def main():
    directory = Path(sys.argv[1] if len(sys.argv) > 1 else "target/binding-parity/python").resolve()
    directory.mkdir(parents=True, exist_ok=True)
    style = RenderStyle(
        font=FontFamilies(family="monospace"), font_size=24, canvas_padding=0,
        content_padding=PaddingSides(left=2), window=WindowStyle(title_bar=False),
        border=BorderStyle(radius=0), shadow=ShadowStyle(enabled=False),
        cursor=RecordingCursorStyle(blink=False, blink_interval_ms=250),
    )
    original = deepcopy(style)
    async with TuiTest.ephemeral("rendering", style=style) as terminal:
        await terminal.run(sys.executable, "-u", "-c",
                           "print('\\x1b[5 q\\x1b]10;#abcdef\\x07rendering-ready', end='', flush=True); input()",
                           cols=30, rows=3)
        await terminal.get_by_text("rendering-ready").expect()
        await terminal.expect_cursor(visible=True, shape="bar")
        await terminal.expect_mode("cursor_visible", True)
        await terminal.expect_colors(foreground="#abcdef")
        assert await terminal.get_modes() == (await terminal.state()).modes
        assert await terminal.get_colors() == (await terminal.state()).colors
        path = directory / "custom.svg"
        await terminal.screenshot(str(path))
        svg = path.read_text()
        assert 'font-size="24px"' in svg and 'font-family="monospace"' in svg
        await terminal.restart(graceful_timeout=0)
        await terminal.get_by_text("rendering-ready").expect()
        await terminal.screenshot(str(path))
        assert path.read_text() == svg
        assert style == original
        await terminal.start_recording(str(directory / "steady.gif"), fps=30)
        # Set playback duration after the readiness check.
        await asyncio.sleep(1.1)
        await terminal.stop_recording()
        print(f"{path}: custom font and geometry survive restart; cursor blink disabled")
        print("Native cursor, mode, and color assertions passed; caller settings unchanged")


if __name__ == "__main__":
    asyncio.run(main())
