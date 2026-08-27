import asyncio
import re
import unittest

from tui_test import _config as cfg
from tui_test import _ephemeral as ephemeral
from tui_test import client
from tui_test.errors import ExpectationError, TerminalArtifact
from tui_test.types import Colors, Profile, Timeouts


def run(coro):
    return asyncio.run(coro)


class _FakeNative:
    def __init__(self):
        self.calls = []
        self.reply = {}
        self.error = None

    def __getattr__(self, name):
        def invoke(*args):
            self.calls.append((name, args))

            async def complete():
                if self.error is not None:
                    raise self.error
                return self.reply

            return complete()

        return invoke


class _CapturingClient(client.TuiTest):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.fake = _FakeNative()
        self._native = self.fake


class TimeoutResolutionTests(unittest.TestCase):
    def test_returns_none_when_nothing_configured(self):
        for class_name in ("text", "idle", "command", "exit", "ready"):
            self.assertIsNone(cfg.resolve_timeout(class_name))

    def test_per_call_beats_timeouts_field(self):
        self.assertEqual(
            cfg.resolve_timeout("command", call=111, timeouts={"command": 222}),
            111,
        )

    def test_timeouts_field_used_when_no_call(self):
        self.assertEqual(
            cfg.resolve_timeout("command", timeouts={"command": 222}), 222
        )

    def test_none_entry_in_timeouts_falls_through(self):
        self.assertIsNone(cfg.resolve_timeout("text", timeouts={"text": None}))

    def test_normalize_timeouts_accepts_dataclass_and_mapping(self):
        self.assertIsNone(cfg.normalize_timeouts(None))
        self.assertEqual(cfg.normalize_timeouts({"command": 42})["command"], 42)
        normalized = cfg.normalize_timeouts(Timeouts(command=42))
        self.assertEqual(normalized["command"], 42)
        self.assertIsNone(normalized["idle"])

    def test_session_timeouts_payload_omits_when_empty(self):
        self.assertIsNone(cfg.session_timeouts_payload(None))
        self.assertIsNone(cfg.session_timeouts_payload({}))
        self.assertIsNone(cfg.session_timeouts_payload(Timeouts()))

    def test_session_timeouts_payload_keeps_only_set_classes(self):
        self.assertEqual(
            cfg.session_timeouts_payload(
                Timeouts(command=2000, ready=3000)
            ),
            {"command": 2000, "ready": 3000},
        )


class ProfileResolutionTests(unittest.TestCase):
    def test_normalize_accepts_dataclass_and_mapping(self):
        self.assertIsNone(cfg.normalize_profile(None))
        self.assertEqual(
            cfg.normalize_profile(
                Profile(scrollback=50, colors=Colors(red="#010203"))
            ),
            {"scrollback": 50, "colors": {"red": "#010203"}},
        )
        self.assertEqual(
            cfg.normalize_profile({"colors": {"bright_blue": "#abc"}}),
            {"colors": {"bright_blue": "#abc"}},
        )

    def test_unknown_profile_fields_are_rejected(self):
        with self.assertRaises(ValueError):
            cfg.normalize_profile({"scrollbacks": 10})
        with self.assertRaises(ValueError):
            cfg.normalize_profile({"colors": {"chartreuse": "#123456"}})


class BackendResolutionTests(unittest.TestCase):
    def test_normalizes_backend_names(self):
        self.assertIsNone(cfg.normalize_backend(None))
        self.assertEqual(cfg.normalize_backend("alacritty"), "alacritty")
        self.assertEqual(cfg.normalize_backend("ghostty"), "ghostty")
        self.assertEqual(cfg.normalize_backend("rio"), "rio")

    def test_rejects_unknown_backend(self):
        for backend in ("xterm", "libghostty"):
            with self.assertRaises(ValueError):
                cfg.normalize_backend(backend)


class TypedCallTests(unittest.TestCase):
    def test_open_uses_typed_arguments(self):
        terminal = _CapturingClient("s")
        run(
            terminal.open(
                cols=120,
                rows=40,
                env={"K": "V"},
                restart=True,
                profile=Profile(
                    scrollback=321,
                    colors=Colors(red="#010203"),
                ),
                timeouts=Timeouts(text=100, ready=200),
            )
        )
        name, args = terminal.fake.calls[0]
        self.assertEqual(name, "open")
        self.assertEqual(
            args[:7], (None, None, 120, 40, None, [("K", "V")], None)
        )
        self.assertTrue(args[7])
        self.assertEqual(args[8], 321)
        self.assertEqual(args[9], [("red", "#010203")])
        self.assertEqual(args[10:], (100, None, None, None, 200))

    def test_run_uses_program_and_argv(self):
        terminal = _CapturingClient("s")
        run(terminal.run("vim", "file.txt"))
        name, args = terminal.fake.calls[0]
        self.assertEqual(name, "run")
        self.assertEqual(args[0], "vim")
        self.assertEqual(args[1], ["file.txt"])

    def test_constructor_profile_is_forwarded_to_run(self):
        terminal = _CapturingClient(
            "s", profile=Profile(colors=Colors(background="#112233"))
        )
        run(terminal.run("vim"))
        args = terminal.fake.calls[0][1]
        self.assertFalse(args[8])
        self.assertIsNone(args[9])
        self.assertEqual(args[10], [("background", "#112233")])

    def test_constructor_and_call_backends_are_forwarded(self):
        terminal = _CapturingClient("s", backend="ghostty")
        run(terminal.open())
        run(terminal.run("vim", backend="alacritty"))
        self.assertEqual(terminal.fake.calls[0][1][1], "ghostty")
        self.assertEqual(terminal.fake.calls[1][1][2], "alacritty")

    def test_input_helpers_use_distinct_typed_methods(self):
        terminal = _CapturingClient("s")
        run(terminal.type("typed"))
        run(terminal.write("written"))
        run(terminal.submit("echo hi"))
        run(terminal.press("Enter"))
        run(terminal.keyboard.press("Escape"))
        run(terminal.keyboard.down("Escape", "Enter"))
        run(terminal.keyboard.repeat("Enter"))
        run(terminal.keyboard.up("Escape", "Enter"))
        self.assertEqual(
            terminal.fake.calls,
            [
                ("type", ("typed",)),
                ("write", ("written",)),
                ("submit", ("echo hi",)),
                ("press", (["Enter"],)),
                ("press", (["Escape"],)),
                ("key_down", (["Escape", "Enter"],)),
                ("repeat", (["Enter"],)),
                ("key_up", (["Escape", "Enter"],)),
            ],
        )

    def test_mouse_helpers_use_typed_methods(self):
        terminal = _CapturingClient("s")
        run(
            terminal.mouse.click(
                on_text="OK",
                button="right",
                alt=True,
                ctrl=True,
                shift=True,
                clicks=2,
            )
        )
        run(terminal.mouse.move(1, 2))
        run(terminal.mouse.down(1, 2, button="middle", ctrl=True))
        run(terminal.mouse.up(1, 2, button="right", alt=True))
        run(terminal.mouse.drag(1, 2, 3, 4, shift=True))
        run(terminal.mouse.scroll("down", amount=4))
        self.assertEqual(
            terminal.fake.calls,
            [
                ("mouse_click", (None, None, "OK", 30, 2)),
                ("mouse_move", (1, 2)),
                ("mouse_down", (1, 2, 17)),
                ("mouse_up", (1, 2, 10)),
                ("mouse_drag", (1, 2, 3, 4, 4)),
                ("mouse_scroll", ("down", 4)),
            ],
        )

    def test_mouse_helpers_reject_invalid_options(self):
        terminal = _CapturingClient("s")
        with self.assertRaisesRegex(ValueError, "unknown mouse button"):
            run(terminal.mouse.click(0, 0, button="primary"))
        with self.assertRaisesRegex(TypeError, "button must be a string"):
            run(terminal.mouse.click(0, 0, button=1))
        with self.assertRaisesRegex(TypeError, "ctrl must be a bool"):
            run(terminal.mouse.click(0, 0, ctrl=1))

    def test_typed_getters_use_distinct_native_methods(self):
        terminal = _CapturingClient("s")
        for method in (
            terminal.get_command,
            terminal.get_output,
            terminal.get_exit_code,
            terminal.get_cwd,
            terminal.get_cursor,
            terminal.get_size,
        ):
            run(method())
        self.assertEqual(
            [name for name, _ in terminal.fake.calls],
            [
                "get_command",
                "get_output",
                "get_exit_code",
                "get_cwd",
                "get_cursor",
                "get_size",
            ],
        )
        self.assertFalse(hasattr(client.TuiTest, "send"))
        self.assertFalse(hasattr(client.TuiTest, "get"))

    def test_recording_helpers_use_typed_methods(self):
        terminal = _CapturingClient("s")
        run(
            terminal.start_recording(
                "demo.png",
                format="apng",
                fps=24,
                speed=2.0,
                idle_time_limit=3.0,
                zoom=0.5,
            )
        )
        run(terminal.stop_recording())
        self.assertEqual(
            terminal.fake.calls,
            [
                (
                    "start_recording",
                    ("demo.png", "apng", 24, 2.0, 3.0, 0.5),
                ),
                ("stop_recording", ()),
            ],
        )

    def test_screenshot_forwards_zoom(self):
        terminal = _CapturingClient("s")
        run(terminal.screenshot("screen.svg", full=True, zoom=0.5))
        self.assertEqual(
            terminal.fake.calls,
            [("screenshot", ("screen.svg", True, 0.5))],
        )

    def test_screenshot_rejects_zoom_without_path(self):
        terminal = _CapturingClient("s")
        with self.assertRaisesRegex(ValueError, "requires a path"):
            run(terminal.screenshot(zoom=0.5))


class ClientTimeoutTests(unittest.TestCase):
    def test_unconfigured_waits_pass_none(self):
        terminal = _CapturingClient("s")
        run(terminal.wait_idle())
        run(terminal.wait_command())
        run(terminal.wait_exit())
        run(terminal.wait_ready())
        self.assertEqual(
            terminal.fake.calls,
            [
                ("wait_idle", (None,)),
                ("wait_command", (None,)),
                ("wait_exit", (None,)),
                ("wait_ready", (None,)),
            ],
        )

    def test_client_and_per_call_timeouts_resolve(self):
        terminal = _CapturingClient(
            "s", timeouts=Timeouts(text=1234, command=2222, idle=1500)
        )
        run(terminal.wait_text("x"))
        run(terminal.wait_idle(timeout=50))
        run(terminal.expect_exit_code(0))
        self.assertEqual(terminal.fake.calls[0][1][-1], 1234)
        self.assertEqual(terminal.fake.calls[1], ("wait_idle", (50,)))
        self.assertEqual(
            terminal.fake.calls[2], ("expect_exit_code", (0, 2222))
        )

    def test_open_and_run_forward_session_timeouts(self):
        terminal = _CapturingClient("s")
        run(terminal.open(timeouts=Timeouts(text=1000, ready=2000)))
        run(terminal.run("vim", timeouts=Timeouts(idle=1500)))
        self.assertEqual(
            terminal.fake.calls[0][1][-5:], (1000, None, None, None, 2000)
        )
        self.assertEqual(
            terminal.fake.calls[1][1][-5:], (None, 1500, None, None, None)
        )


class RetryTests(unittest.TestCase):
    def test_retries_reattempt_and_reraise_last(self):
        terminal = _CapturingClient("s")
        attempts = {"count": 0}

        def open_call(*args):
            terminal.fake.calls.append(("open", args))

            async def complete():
                attempts["count"] += 1
                raise RuntimeError("attempt %d" % attempts["count"])

            return complete()

        terminal.fake.open = open_call
        with self.assertRaises(RuntimeError) as raised:
            run(terminal.open(retries=2))
        self.assertEqual(attempts["count"], 3)
        self.assertEqual(str(raised.exception), "attempt 3")

    def test_no_retries_single_attempt(self):
        terminal = _CapturingClient("s")
        terminal.fake.error = RuntimeError("boom")
        with self.assertRaises(RuntimeError):
            run(terminal.open())
        self.assertEqual(
            len([call for call in terminal.fake.calls if call[0] == "open"]),
            1,
        )


class MessagePrefixTests(unittest.TestCase):
    def _prefix_for(self, method_name, *args):
        terminal = _CapturingClient("s")
        terminal.fake.error = ExpectationError("boom")
        with self.assertRaises(ExpectationError) as raised:
            run(getattr(terminal, method_name)(*args))
        return str(raised.exception)

    def test_all_wait_and_expect_methods_prefix(self):
        cases = {
            "wait_text": ("x",),
            "wait_idle": (),
            "wait_command": (),
            "wait_exit": (),
            "wait_ready": (),
            "expect_text": ("x",),
            "expect_exit_code": (0,),
            "expect_output": ("x",),
            "expect_snapshot": ("x",),
        }
        for name, args in cases.items():
            self.assertTrue(
                self._prefix_for(name, *args).startswith(name + ": ")
            )


class ArtifactCaptureTests(unittest.TestCase):
    def test_text_mode_captures_terminal_text_only(self):
        terminal = _CapturingClient(
            "s", artifacts={"dir": "unused", "on_failure": "text"}
        )
        terminal.fake.error = ExpectationError(
            "nope\n\nTerminal content:\n╭──╮\n╰──╯"
        )
        with self.assertRaises(ExpectationError) as raised:
            run(terminal.wait_text("x"))
        artifact = raised.exception.terminal
        self.assertIsInstance(artifact, TerminalArtifact)
        self.assertIn("╭──╮", artifact.text)
        self.assertIsNone(artifact.screenshot)

    def test_capture_never_masks_original_error(self):
        terminal = _CapturingClient(
            "s", artifacts={"dir": "unused", "on_failure": "svg"}
        )
        terminal.fake.error = ExpectationError(
            "nope\n\nTerminal content:\n╭──╮\n╰──╯"
        )

        async def boom(*args, **kwargs):
            raise RuntimeError("screenshot exploded")

        terminal.screenshot = boom
        with self.assertRaises(ExpectationError):
            run(terminal.wait_text("x"))


class UniqueSessionTests(unittest.TestCase):
    def test_format_and_uniqueness(self):
        first = ephemeral.unique_session()
        second = ephemeral.unique_session()
        self.assertTrue(first.startswith("tui-test-"))
        self.assertNotEqual(first, second)

    def test_sanitizes_and_caps_names(self):
        name = ephemeral.unique_session("a b/c\\d:e.f")
        self.assertIsNotNone(re.fullmatch(r"[A-Za-z0-9_-]+", name))
        long_name = ephemeral.unique_session("x" * 500)
        self.assertLessEqual(len(long_name), 64)
        self.assertRegex(long_name, r"-\d+-[0-9a-f]+-\d+$")


class UnknownTimeoutClassTests(unittest.TestCase):
    def test_normalize_rejects_unknown_keys(self):
        with self.assertRaises(ValueError) as raised:
            cfg.normalize_timeouts({"comand": 100})
        self.assertIn("comand", str(raised.exception))

    def test_open_rejects_unknown_keys(self):
        with self.assertRaises(ValueError):
            run(_CapturingClient("s").open(timeouts={"txt": 100}))


if __name__ == "__main__":
    unittest.main()
