import asyncio
import gc
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import shell_use
from shell_use import (
    ExpectationError,
    InternalError,
    NoSessionError,
    ShellUse,
    Timeouts,
    UsageError,
    get_recording,
    testing,
    unique_session,
)
from shell_use.client import _panic_probe

SHELL = "pwsh" if sys.platform == "win32" else None
TWO_BELLS_COMMAND = (
    "[Console]::Out.Write([char]7); [Console]::Out.Write([char]7)"
    if sys.platform == "win32"
    else "printf '\\a\\a'"
)
DELAYED_BELL_COMMAND = (
    "Start-Sleep -Seconds 1; [Console]::Out.Write([char]7)"
    if sys.platform == "win32"
    else "sleep 1; printf '\\a'"
)


def run(coro):
    return asyncio.run(coro)


class IntegrationTests(unittest.TestCase):
    def _client(self):
        return ShellUse.ephemeral("pytest")

    def test_echo_roundtrip(self):
        async def scenario():
            async with self._client() as su:
                await su.open(shell=SHELL)
                await su.submit("echo hello-sdk")
                await su.wait_command()
                await su.expect_text("hello-sdk", strict=False)
                await su.expect_exit_code(0)
                st = await su.state()
                self.assertGreater(st.cols, 0)

        run(scenario())

    def test_invalid_shell_is_a_typed_usage_error(self):
        async def scenario():
            su = self._client()
            try:
                with self.assertRaises(UsageError):
                    await su.open(shell="not-a-real-shell")
            finally:
                await su.close_quiet()

        run(scenario())

    def test_bell_state_waits_and_expectations(self):
        async def scenario():
            async with self._client() as su:
                await su.open(shell=SHELL)

                await su.submit(TWO_BELLS_COMMAND)
                await su.expect_bell_count(2, timeout=5000)
                await su.wait_command()

                self.assertEqual((await su.state()).bell_count, 2)
                self.assertEqual(await su.get_bell_count(), 2)
                self.assertEqual(await su.get_bell_count(), 2)

                await su.submit(DELAYED_BELL_COMMAND)
                await su.wait_bell(timeout=5000)
                await su.expect_bell_count(3)
                self.assertEqual(await su.get_bell_count(), 3)

        run(scenario())

    def test_effective_timeouts_are_exposed_in_typed_state(self):
        async def scenario():
            expected = Timeouts(
                text=1234,
                idle=2345,
                command=3456,
                exit=4567,
                ready=5678,
            )
            async with self._client() as su:
                await su.open(shell=SHELL, timeouts=expected)
                self.assertEqual((await su.state()).timeouts, expected)

        run(scenario())

    def test_invalid_numeric_arguments_are_typed_usage_errors(self):
        async def scenario():
            su = self._client()
            cases = [
                ("u16-negative", lambda: su.resize(-1, 24)),
                ("u16-too-large", lambda: su.cells(2**16, 0)),
                ("u16-bool", lambda: su.resize(True, 24)),
                ("u8-negative", lambda: su.mouse.down(0, 0, button=-1)),
                ("u8-too-large", lambda: su.mouse.down(0, 0, button=2**8)),
                ("u8-bool", lambda: su.mouse.down(0, 0, button=True)),
                ("u64-negative", lambda: su.wait_idle(timeout=-1)),
                ("u64-too-large", lambda: su.wait_idle(timeout=2**64)),
                ("u64-huge", lambda: su.wait_idle(timeout=10**1000)),
                ("u64-bool", lambda: su.wait_idle(timeout=True)),
                (
                    "i32-too-small",
                    lambda: su.expect_exit_code(-(2**31) - 1),
                ),
                ("i32-too-large", lambda: su.expect_exit_code(2**31)),
                ("i32-bool", lambda: su.expect_exit_code(True)),
                ("non-integer", lambda: su.resize(object(), 24)),
            ]
            for label, call in cases:
                with self.subTest(label=label):
                    with self.assertRaises(UsageError) as raised:
                        await call()
                    self.assertIn("must be an integer", str(raised.exception))

        run(scenario())

    def test_expect_text_error_includes_terminal(self):
        async def scenario():
            async with self._client() as su:
                await su.run(
                    sys.executable,
                    "-c",
                    "import sys,time; sys.stdout.write('ready'); "
                    "sys.stdout.flush(); time.sleep(60)",
                )
                await su.wait_text("ready", timeout=2000)
                with self.assertRaises(ExpectationError) as raised:
                    await su.expect_text("text-that-is-not-on-screen", timeout=50)
                message = str(raised.exception)
                self.assertIn(
                    "expect_text: timed out after 50ms waiting for "
                    "'text-that-is-not-on-screen' to be visible",
                    message,
                )
                self.assertIn("Terminal content:\n╭", message)
                self.assertIn("ready", message)
                self.assertIn("\n╰", message)

        run(scenario())

    def test_blocking_wait_does_not_block_event_loop(self):
        async def scenario():
            async with self._client() as su:
                await su.open(shell=SHELL)
                ticks = []
                stop = asyncio.Event()

                async def heartbeat():
                    while not stop.is_set():
                        ticks.append(asyncio.get_running_loop().time())
                        await asyncio.sleep(0.01)

                heartbeat_task = asyncio.create_task(heartbeat())
                try:
                    with self.assertRaises(ExpectationError):
                        await su.wait_text(
                            "text-that-will-never-appear-on-screen", timeout=300
                        )
                finally:
                    stop.set()
                    await heartbeat_task

                self.assertGreaterEqual(len(ticks), 5)

        run(scenario())

    def test_sessions_lists_open_session(self):
        async def scenario():
            su = ShellUse(unique_session("pytest"))
            await su.open(shell=SHELL)
            try:
                names = await shell_use.sessions()
                self.assertIn(su.session, names)
            finally:
                await su.close_quiet()

        run(scenario())

    def test_close_evicts_session_and_retains_recording(self):
        async def scenario():
            name = unique_session("recording")
            su = ShellUse(name)
            await su.open(shell=SHELL)
            await su.submit("echo retained-recording")
            await su.wait_command()
            await su.close()

            self.assertNotIn(name, await shell_use.sessions())
            with self.assertRaises(NoSessionError):
                await su.state()
            self.assertIn("retained-recording", await get_recording(name))
            with self.assertRaises(NoSessionError):
                await get_recording(unique_session("missing-recording"))

        run(scenario())

    def test_same_name_clients_share_typed_operations(self):
        async def scenario():
            name = unique_session("same-name")
            first = ShellUse(name)
            second = ShellUse(name)
            try:
                await first.open(shell=SHELL)
                await second.submit("echo shared-session")
                await first.wait_command()
                self.assertIn("shared-session", await second.text())
                self.assertIn(name, await shell_use.sessions())
            finally:
                await first.close_quiet()
                await second.close_quiet()

        run(scenario())

    def test_close_all_cleans_process_local_sessions(self):
        async def scenario():
            first = self._client()
            second = self._client()
            await first.open(shell=SHELL)
            await second.open(shell=SHELL)
            await shell_use.close_all()
            self.assertNotIn(first.session, await shell_use.sessions())
            self.assertNotIn(second.session, await shell_use.sessions())
            with self.assertRaises(NoSessionError):
                await first.state()
            with self.assertRaises(NoSessionError):
                await second.state()

        run(scenario())

    def test_close_all_interrupts_in_flight_waits(self):
        async def scenario():
            su = self._client()
            await su.open(shell=SHELL)
            wait = asyncio.create_task(
                su.wait_text("never-visible", timeout=60_000)
            )
            await asyncio.sleep(0.05)

            await asyncio.wait_for(shell_use.close_all(), timeout=2)
            with self.assertRaises(ExpectationError) as raised:
                await wait
            self.assertIn("session exited before", str(raised.exception))
            self.assertNotIn(su.session, await shell_use.sessions())

        run(scenario())

    def test_typed_operation_results_keep_public_shapes(self):
        async def scenario():
            async with self._client() as su:
                opened = await su.open(shell=SHELL, cols=92, rows=28)
                self.assertEqual(opened["session"], su.session)
                self.assertIn("ready", opened)
                await su.resize(90, 27)
                await su.type("echo typed-input")
                await su.press("Enter")
                await su.wait_command()
                await su.expect_output("typed-input", regex=False)
                await su.expect_text("typed-input", strict=False)
                self.assertEqual(await su.get_exit_code(), 0)
                self.assertIn("echo typed-input", await su.get_command())
                self.assertIn("typed-input", await su.get_output())
                self.assertIsInstance(await su.get_cwd(), (str, type(None)))
                self.assertEqual(await su.get_size(), {"cols": 90, "rows": 27})
                cursor = await su.get_cursor()
                self.assertEqual(set(cursor), {"x", "y"})
                cells = await su.cells(0, 0, 2, 1)
                self.assertTrue(cells)
                self.assertIsInstance(cells[0].fg, (str, int))
                self.assertIsInstance(cells[0].bg, (str, int))
                self.assertIn("typed-input", await su.screenshot())
                await su.mouse.click(0, 0)
                await su.mouse.move(1, 1)
                await su.mouse.down(1, 1)
                await su.mouse.up(1, 1)
                await su.mouse.drag(0, 0, 1, 1)
                await su.mouse.scroll("down", amount=1)

        run(scenario())

    def test_signal_and_wait_exit_are_typed_operations(self):
        async def scenario():
            async with self._client() as su:
                await su.run(
                    sys.executable,
                    "-c",
                    "import time; print('signal-ready', flush=True); time.sleep(60)",
                )
                await su.wait_text("signal-ready", timeout=5000)
                with self.assertRaises(ExpectationError):
                    await su.wait_exit(timeout=30)
                await su.signal("KILL")

        run(scenario())

    def test_packed_screen_preserves_logical_utf8_rows(self):
        async def scenario():
            su = self._client()
            await su.run(
                sys.executable,
                "-c",
                "import sys,time; "
                "sys.stdout.buffer.write("
                "bytes.fromhex('c3a9e7958c5820200d0a0d0a5a')); "
                "sys.stdout.flush(); time.sleep(60)",
                cols=8,
                rows=4,
                wait_ready=False,
            )
            await su.wait_text("X", timeout=5000)
            view, cols, rows = await su._packed_screen()
            self.assertIsInstance(view, memoryview)
            self.assertTrue(view.readonly)
            self.assertEqual((cols, rows), (8, 4))
            before = bytes(view)
            text = before.decode("utf-8")
            lines = text.split("\n")
            self.assertEqual(len(lines), rows)
            self.assertTrue(lines[0].startswith("é界X"))
            self.assertTrue(lines[0].endswith(" "))
            self.assertEqual(lines[1], " " * cols)
            self.assertTrue(lines[2].startswith("Z"))
            self.assertEqual(lines[3], " " * cols)

            x_byte_offset = before.index(b"X")
            self.assertEqual(x_byte_offset, len("é界".encode("utf-8")))
            self.assertNotEqual(x_byte_offset, 3)
            self.assertEqual((await su.cells(3, 0))[0].char, "X")

            await su.close()
            del su
            gc.collect()
            self.assertEqual(bytes(view), before)
            if len(view):
                with self.assertRaises(TypeError):
                    view[0] = 0

        run(scenario())

    def test_panic_probe_maps_to_internal_error_and_process_survives(self):
        async def scenario():
            with self.assertRaises(InternalError) as raised:
                await _panic_probe()
            self.assertIn("panic probe", str(raised.exception))
            async with self._client() as su:
                await su.open(shell=SHELL)
                self.assertGreater((await su.state()).cols, 0)

        run(scenario())

    def test_cancelling_wait_keeps_native_operation_serialized(self):
        async def scenario():
            async with self._client() as su:
                await su.run(
                    sys.executable,
                    "-c",
                    "import time; print('cancel-ready', flush=True); time.sleep(60)",
                )
                await su.wait_text("cancel-ready", timeout=5000)
                wait = asyncio.create_task(
                    su.wait_text("never-visible", timeout=350)
                )
                await asyncio.sleep(0.05)
                wait.cancel()
                with self.assertRaises(asyncio.CancelledError):
                    await wait

                started = asyncio.get_running_loop().time()
                state = await su.state()
                elapsed = asyncio.get_running_loop().time() - started
                self.assertGreater(state.cols, 0)
                self.assertGreaterEqual(elapsed, 0.15)

        run(scenario())

    def test_any_shared_handle_can_close_a_reopened_named_session(self):
        async def scenario():
            name = unique_session("shared-close")
            first = ShellUse(name)
            second = ShellUse(name)
            try:
                await first.open(shell=SHELL)
                await first.close()
                await second.open(shell=SHELL)
                await first.close()
                self.assertNotIn(name, await shell_use.sessions())
            finally:
                await second.close_quiet()

        run(scenario())

    def test_snapshot_lands_in_client_cwd(self):
        async def scenario():
            original = os.getcwd()
            snap_root = tempfile.mkdtemp(prefix="shell-use-snap-")
            name = f"snap-{os.path.basename(snap_root)}"
            try:
                async with self._client() as su:
                    await su.open(shell=SHELL)
                    await su.submit("echo snapshot-marker")
                    await su.wait_command()
                    await su.wait_idle()
                    os.chdir(snap_root)
                    try:
                        status = await su.expect_snapshot(name)
                        self.assertEqual(status, "written")
                        created = Path(snap_root) / "__snapshots__" / f"{name}.snap"
                        self.assertTrue(created.is_file())
                        other_cwd = Path(original) / "__snapshots__" / f"{name}.snap"
                        self.assertFalse(other_cwd.exists())
                        self.assertEqual(await su.expect_snapshot(name), "passed")
                    finally:
                        os.chdir(original)
            finally:
                shutil.rmtree(snap_root, ignore_errors=True)

        run(scenario())


class TestingHelperTests(unittest.TestCase):
    def tearDown(self):
        run(testing.close_all_tracked())
        testing.reset_terminal_defaults()

    def test_terminal_drives_a_real_shell_and_cleans_up(self):
        async def scenario():
            async with testing.terminal(shell=SHELL) as t:
                session = t.session
                await t.submit("echo helper-sdk")
                await t.wait_command()
                await t.expect_text("helper-sdk", strict=False)
                await t.expect_exit_code(0)
                self.assertEqual(testing.tracked_count(), 1)
            self.assertEqual(testing.tracked_count(), 0)
            self.assertNotEqual(session, "default")

        run(scenario())

    def test_create_terminal_is_tracked_until_closed(self):
        async def scenario():
            t = await testing.create_terminal(shell=SHELL)
            self.assertEqual(testing.tracked_count(), 1)
            await testing.close_all_tracked()
            self.assertEqual(testing.tracked_count(), 0)
            await t.close_quiet()

        run(scenario())

    def test_two_terminals_are_isolated(self):
        async def scenario():
            async with testing.terminal(shell=SHELL) as a:
                async with testing.terminal(shell=SHELL) as b:
                    self.assertNotEqual(a.session, b.session)
                    await a.submit("echo only-in-a")
                    await a.wait_command()
                    await b.expect_text("only-in-a", not_=True)

        run(scenario())

    def test_program_option_uses_run(self):
        async def scenario():
            async with testing.terminal(
                program=[
                    sys.executable,
                    "-c",
                    "import sys,time; sys.stdout.write('from-run'); "
                    "sys.stdout.flush(); time.sleep(60)",
                ]
            ) as t:
                await t.wait_text("from-run", timeout=5000)

        run(scenario())

    def test_terminal_snapshot_normalises_live_output(self):
        async def scenario():
            async with testing.terminal(shell=SHELL) as t:
                await t.submit("echo snap-me")
                await t.wait_command()
                normalised = testing.terminal_snapshot(await t.text())
                self.assertIn("snap-me", normalised)
                self.assertFalse(normalised.endswith("\n"))
                for line in normalised.split("\n"):
                    self.assertEqual(line, line.rstrip())

        run(scenario())

    def test_suite_defaults_reach_the_client(self):
        async def scenario():
            testing.set_terminal_defaults(cols=101, rows=24)
            async with testing.terminal(shell=SHELL) as t:
                state = await t.state()
                self.assertEqual(state.cols, 101)
                self.assertEqual(state.rows, 24)

        run(scenario())


if __name__ == "__main__":
    unittest.main()
