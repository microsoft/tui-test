import asyncio
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import shell_use
from shell_use import (
    ExpectationError,
    NoSessionError,
    ShellUse,
    UsageError,
    get_recording,
    testing,
    unique_session,
)

SHELL = "pwsh" if sys.platform == "win32" else None


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

    def test_cli_control_requests_are_rejected(self):
        async def scenario():
            async with self._client() as su:
                await su.open(shell=SHELL)
                with self.assertRaises(UsageError):
                    await su.send({"kind": "shutdown"})
                self.assertGreater((await su.state()).cols, 0)

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
            with self.assertRaises(UsageError):
                await su.send({"kind": "shutdown"})
            self.assertIn("retained-recording", await get_recording(name))
            with self.assertRaises(NoSessionError):
                await get_recording(unique_session("missing-recording"))

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
