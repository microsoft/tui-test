"""Run with an installed wheel or TUI_TEST_NATIVE_LIBRARY."""

import asyncio
import sys

from tui_test import TuiTest


async def main():
    async with TuiTest.ephemeral("shared-library") as terminal:
        await terminal.run(
            sys.executable, "-u", "-c",
            "print('Shared typed ABI: Python'); input()",
        )
        await terminal.get_by_text("Shared typed ABI: Python").expect()
        packed, cols, rows = await terminal._packed_screen()
    assert packed.readonly
    assert b"Shared typed ABI: Python" in packed.tobytes()
    print("Shared library: {} columns, {} rows".format(cols, rows))
    print("Copied screen remains valid after close:")
    print(packed.tobytes().decode("utf-8").strip())


if __name__ == "__main__":
    asyncio.run(main())
