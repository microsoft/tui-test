import asyncio
import unittest
from unittest import mock

from tui_test import testing
from tui_test.types import Colors, Profile


def run(coro):
    return asyncio.run(coro)


class _FakeTerminal:
    def __init__(self):
        self.closed = 0

    async def close_quiet(self):
        self.closed += 1


class TerminalSnapshotTests(unittest.TestCase):
    def test_trims_trailing_whitespace_per_line(self):
        self.assertEqual(testing.terminal_snapshot("a   \nb\t\n"), "a\nb")

    def test_drops_trailing_blank_lines(self):
        self.assertEqual(testing.terminal_snapshot("a\n\n\n\n"), "a")

    def test_keeps_interior_blank_lines(self):
        self.assertEqual(testing.terminal_snapshot("a\n\nb\n"), "a\n\nb")

    def test_all_blank_collapses_to_empty(self):
        self.assertEqual(testing.terminal_snapshot("\n  \n\n"), "")


class DefaultShellTests(unittest.TestCase):
    def test_matches_the_engine_default(self):
        import sys

        if sys.platform == "win32":
            expected = "powershell"
        elif sys.platform == "darwin":
            expected = "zsh"
        else:
            expected = "bash"
        self.assertEqual(testing.DEFAULT_SHELL, expected)


class RegistryTests(unittest.TestCase):
    def tearDown(self):
        run(testing.close_all_tracked())

    def test_tracking_and_closing(self):
        a, b = _FakeTerminal(), _FakeTerminal()
        testing.track_terminal(a)
        testing.track_terminal(b)
        self.assertEqual(testing.tracked_count(), 2)
        run(testing.close_all_tracked())
        self.assertEqual((a.closed, b.closed), (1, 1))
        self.assertEqual(testing.tracked_count(), 0)

    def test_untracking_exempts_a_terminal(self):
        a = _FakeTerminal()
        testing.track_terminal(a)
        testing.untrack_terminal(a)
        run(testing.close_all_tracked())
        self.assertEqual(a.closed, 0)

    def test_untracking_an_unknown_terminal_is_safe(self):
        testing.untrack_terminal(_FakeTerminal())


class SafetyNetTests(unittest.TestCase):
    def test_registers_the_terminal_closer_at_exit(self):
        calls = []
        installed = testing._safety_net_installed
        testing._safety_net_installed = False
        try:
            with mock.patch.object(
                testing.atexit,
                "register",
                side_effect=lambda callback: calls.append(callback.__name__),
            ):
                testing._install_safety_net()
        finally:
            testing._safety_net_installed = installed

        self.assertEqual(calls, ["_close_all_tracked_blocking"])


class DefaultsTests(unittest.TestCase):
    def tearDown(self):
        testing.reset_terminal_defaults()

    def test_defaults_are_merged_and_reset(self):
        testing.set_terminal_defaults(retries=5, cols=101)
        defaults = testing.get_terminal_defaults()
        self.assertEqual(defaults.retries, 5)
        self.assertEqual(defaults.cols, 101)
        testing.reset_terminal_defaults()
        self.assertIsNone(testing.get_terminal_defaults().cols)

    def test_unknown_option_is_rejected(self):
        with self.assertRaises(TypeError) as raised:
            testing.set_terminal_defaults(binry="/typo")
        self.assertIn("binry", str(raised.exception))


class OptionPlumbingTests(unittest.TestCase):
    def tearDown(self):
        testing.reset_terminal_defaults()

    def test_terminal_options_has_no_removed_process_fields(self):
        self.assertFalse(hasattr(testing.TerminalOptions(), "home"))
        self.assertFalse(hasattr(testing.TerminalOptions(), "isolated"))
        self.assertFalse(hasattr(testing.TerminalOptions(), "binary"))

    def test_retries_default_to_two(self):
        self.assertEqual(testing._spawn_kwargs(testing.TerminalOptions())["retries"], 2)
        self.assertEqual(
            testing._spawn_kwargs(testing.TerminalOptions(retries=0))["retries"], 0
        )

    def test_unset_spawn_options_are_omitted(self):
        kwargs = testing._spawn_kwargs(testing.TerminalOptions())
        self.assertEqual(set(kwargs), {"retries"})

    def test_per_call_options_win_over_defaults(self):
        created = []

        class FakeTuiTest:
            def __init__(self, session, **kwargs):
                self.session = session
                self.kwargs = kwargs
                self.open_kwargs = None
                created.append(self)

            async def open(self, **kwargs):
                self.open_kwargs = kwargs

            async def close_quiet(self):
                pass

        profile = Profile(colors=Colors(red="#010203"))
        testing.set_terminal_defaults(
            cols=100,
            profile=profile,
            artifacts={"dir": "from-defaults"},
        )
        with mock.patch.object(testing, "TuiTest", FakeTuiTest), \
             mock.patch.object(testing, "track_terminal"):
            run(testing.create_terminal(cols=42))
        self.assertEqual(created[0].open_kwargs["cols"], 42)
        self.assertEqual(created[0].kwargs["profile"], profile)
        self.assertEqual(created[0].kwargs["artifacts"], {"dir": "from-defaults"})

    def test_unknown_create_option_is_rejected(self):
        with self.assertRaises(TypeError):
            run(testing.create_terminal(shel="bash"))


if __name__ == "__main__":
    unittest.main()
