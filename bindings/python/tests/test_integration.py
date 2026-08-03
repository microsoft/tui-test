import asyncio
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import shell_use
from shell_use import ExpectationError, ShellUse, testing, unique_session

BIN = os.environ.get("SHELL_USE_BIN")
SHELL = "pwsh" if sys.platform == "win32" else None


def run(coro):
    return asyncio.run(coro)


@unittest.skipUnless(BIN, "set SHELL_USE_BIN to the shell-use binary to run integration tests")
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
                    os.chdir(snap_root)
                    try:
                        status = await su.expect_snapshot(name)
                        self.assertEqual(status, "written")
                        created = Path(snap_root) / "__snapshots__" / f"{name}.snap"
                        self.assertTrue(created.is_file())
                        daemon_side = Path(original) / "__snapshots__" / f"{name}.snap"
                        self.assertFalse(daemon_side.exists())
                        self.assertEqual(await su.expect_snapshot(name), "passed")
                    finally:
                        os.chdir(original)
            finally:
                shutil.rmtree(snap_root, ignore_errors=True)

        run(scenario())


@unittest.skipUnless(BIN, "set SHELL_USE_BIN to the shell-use binary to run integration tests")
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
