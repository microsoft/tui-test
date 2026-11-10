"""Run with PYTHONPATH=bindings/python/src python3 bindings/python/examples/program_status.py."""

import asyncio
import sys

from tui_test import ProgramStatusMatch, TuiTest


async def main():
    async with TuiTest.ephemeral("program-status") as terminal:
        await terminal.run(
            sys.executable, "-u", "-c",
            "import sys; sys.stdout.write('\\x1b]7501;state=working:app=demo\\x07"
            "\\x1b]7501;state=blocked:id=build:kind=permission:progress=50:title=QnVpbGQ=\\x07'); "
            "sys.stdout.flush(); sys.stdin.read()", wait_ready=False,
        )
        await terminal.wait_program_status(ProgramStatusMatch(id="build", state="blocked"))
        await terminal.expect_program_status(ProgramStatusMatch(id="build", app="demo", title="Build"))
        print(await terminal.get_program_statuses())
        print("missing:", await terminal.get_program_status("missing"))


asyncio.run(main())
