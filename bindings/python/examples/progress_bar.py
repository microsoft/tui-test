"""Run with PYTHONPATH=bindings/python/src python3 bindings/python/examples/progress_bar.py."""

import asyncio
import sys

from tui_test import ProgressBarMatch, TuiTest


async def main():
    async with TuiTest.ephemeral("progress-bar") as terminal:
        await terminal.run(
            sys.executable, "-u", "-c",
            "import sys; sys.stdout.write('\\x1b]9;4;1;50\\x07'); sys.stdout.flush(); sys.stdin.read()",
            wait_ready=False,
        )
        await terminal.wait_progress_bar(ProgressBarMatch(state="normal"))
        await terminal.expect_progress_bar(ProgressBarMatch(progress=50))
        print(await terminal.get_progress_bar())


asyncio.run(main())
