"""Run with PYTHONPATH=bindings/python/src python3 bindings/python/examples/parity.py."""

import asyncio
import os
import sys

from tui_test import (
    AutomaticRecording, ExecutionContext, TextAnchor, TextScope, TextStyle,
    TraceOptions, TuiTest, get_recording,
)


async def main():
    directory = os.environ.get("TUI_TEST_PARITY_ARTIFACTS", "artifacts/parity-python")
    terminal = TuiTest.ephemeral("parity", context=ExecutionContext(
        diagnostic_context={"example": "session-parity"},
    ))
    try:
        await terminal.run(sys.executable, "-u", "-c",
                           "import sys; print('Start\\r\\n\\x1b[1mSave\\x1b[0m\\r\\nEnd', flush=True); sys.stdin.read()",
                           wait_ready=False,
                           recording=AutomaticRecording(directory=directory),
                           context=ExecutionContext(
                               diagnostic_context={"phase": "spawn"},
                               trace=TraceOptions(mode="on", directory=directory),
                           ))
        save = terminal.get_by_text(
            "Save", scope=TextScope(after=TextAnchor("Start"), before=TextAnchor("End")),
        ).with_style(TextStyle(bold=True))
        await save.expect()
        composite = save.relative_to(
            terminal.get_by_text("Start"), "after",
        ).with_style(TextStyle(bold=True)).or_(terminal.get_by_link("test:save"))
        assert await composite.count() == 1
        matches = await composite.highlight_matches()
        data, cols, rows = await terminal.packed_screen()
        assert matches[0].text == "Save"
        assert b"Save" in data.tobytes()
        snapshot = await terminal.expect_snapshot("parity", cwd=directory, update=True)
        await terminal.finish_trace(False)
        await terminal.state()
        print(f"scoped bold matches={len(matches)}; packed={cols}x{rows}; snapshot={snapshot}; trace finished; session still open")
        await terminal.interrupt()
        print("pending operations cancelled; child terminated")
        await terminal.close()
        cast = await terminal.run(
            sys.executable, "-u", "-c",
            "import sys; print('cast-only', flush=True); sys.stdin.read()",
            wait_ready=False,
            recording=AutomaticRecording(mode="always", directory=f"{directory}/cast-only"),
        )
        await terminal.get_by_text("cast-only").expect()
        assert "cast-only" in await get_recording(terminal.session)
        print(f"cast-only recording={cast['recording']}")
    finally:
        await terminal.close_quiet()


if __name__ == "__main__":
    asyncio.run(main())
