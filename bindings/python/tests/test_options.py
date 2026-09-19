import asyncio
import json
import os
import re
import unittest
from unittest import mock

import tui_test
from tui_test import (
    FailureArtifactRef,
    FailureArtifactStatus,
    FailureDetails,
    FailureReason,
    _config as cfg,
)
from tui_test import _ephemeral as ephemeral
from tui_test import client
from tui_test.errors import (
    ExpectationError,
    InternalError,
    NoSessionError,
    UsageError,
)
from tui_test.types import AutomaticRecording, Colors, Profile, TextStyle, Timeouts


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


class ClipboardPatternTests(unittest.TestCase):
    def test_compiled_regex_flags_are_encoded(self):
        self.assertEqual(
            client._clipboard_pattern(
                re.compile("ready", re.IGNORECASE | re.MULTILINE | re.DOTALL)
            ),
            ("(?ims:ready)", True),
        )

    def test_unsupported_top_level_and_scoped_flags_are_rejected(self):
        for pattern in (
            re.compile(".", re.ASCII),
            re.compile(r"[ a]", re.VERBOSE),
            re.compile(r"(?a:.)"),
            re.compile(r"(?x:a b)"),
        ):
            with self.subTest(pattern=pattern.pattern):
                with self.assertRaises(ValueError):
                    client._clipboard_pattern(pattern)

    def test_flag_like_text_inside_a_character_class_is_not_rejected(self):
        self.assertEqual(
            client._clipboard_pattern(re.compile(r"[(?x:]")),
            (r"[(?x:]", True),
        )


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


class RecordingResolutionTests(unittest.TestCase):
    def test_accepts_only_directory(self):
        self.assertEqual(
            cfg.normalize_recording(
                AutomaticRecording(directory="casts")
            ),
            {"directory": "casts"},
        )
        with self.assertRaises(ValueError):
            cfg.normalize_recording({"mode": "sometimes"})
        with self.assertRaises(TypeError):
            cfg.normalize_recording({"directory": ""})
        with self.assertRaises(ValueError):
            cfg.normalize_recording({"other": 1})

    def test_trace_modes_are_separate_from_recording(self):
        for mode in ("on", "off", "on-failure"):
            self.assertEqual(cfg.normalize_trace({"mode": mode, "directory": "traces"}), {"mode": mode, "directory": "traces"})
        with self.assertRaises(ValueError):
            cfg.normalize_trace({"mode": "always"})
        with self.assertRaises(TypeError):
            cfg.normalize_trace({"directory": ""})


class FailureDiagnosticsTests(unittest.TestCase):
    def test_native_envelope_is_decoded_and_preserves_cause(self):
        native_error = client.native.NativeAssertionError("native message")
        native_error._tui_test_error_json = json.dumps(
            {
                "kind": "assertion",
                "message": "structured message",
                "details": {
                    "schema_version": 1,
                    "operation": "locator.location",
                    "reason": "locator_no_match",
                    "summary": "missing",
                    "truncated": False,
                    "unknown_additive_field": True,
                },
                "artifact": {
                    "status": "partial",
                    "directory": "artifacts/failure",
                    "report": "artifacts/failure/failure.md",
                    "report_html": "artifacts/failure/failure.html",
                    "timeline": "artifacts/failure/timeline.json",
                    "screen_svg": "artifacts/failure/current.svg",
                    "errors": ["recording omitted"],
                    "unknown_additive_field": True,
                },
            }
        )

        async def fail():
            raise native_error

        with self.assertRaises(ExpectationError) as raised:
            run(client._await_native(fail()))
        error = raised.exception
        self.assertEqual(error.message, "structured message")
        self.assertIs(error.__cause__, native_error)
        self.assertIsInstance(error.details, FailureDetails)
        self.assertEqual(error.details.reason, FailureReason.LOCATOR_NO_MATCH)
        self.assertEqual(error.details.operation, "locator.location")
        self.assertFalse(hasattr(error.details, "terminal"))
        self.assertFalse(hasattr(error.details, "recent_operations"))
        self.assertIsInstance(error.artifact, FailureArtifactRef)
        self.assertEqual(
            error.artifact.status, FailureArtifactStatus.PARTIAL
        )
        self.assertEqual(error.artifact.errors, ("recording omitted",))
        self.assertEqual(error.artifact.report, "artifacts/failure/failure.md")
        self.assertEqual(error.artifact.report_html, "artifacts/failure/failure.html")
        self.assertEqual(error.artifact.timeline, "artifacts/failure/timeline.json")

    def test_current_error_envelopes_can_omit_optional_diagnostics(self):
        for kind, native_type, public_type in (
            ("assertion", client.native.NativeAssertionError, ExpectationError),
            ("usage", client.native.NativeUsageError, UsageError),
            ("no_session", client.native.NativeNoSessionError, NoSessionError),
            ("internal", client.native.NativeInternalError, InternalError),
        ):
            for optional in ({}, {"details": None, "artifact": None}):
                with self.subTest(kind=kind, optional=optional):
                    native_error = native_type("native message")
                    native_error._tui_test_error_json = json.dumps(
                        {"kind": kind, "message": "current message", **optional}
                    )

                    async def fail():
                        raise native_error

                    with self.assertRaises(public_type) as raised:
                        run(client._await_native(fail()))
                    self.assertEqual(raised.exception.message, "current message")
                    self.assertIs(raised.exception.__cause__, native_error)
                    self.assertIsNone(raised.exception.details)
                    self.assertIsNone(raised.exception.artifact)
                    self.assertFalse(hasattr(raised.exception, "terminal"))

    def test_malformed_native_envelope_is_an_internal_transport_error(self):
        valid_details = {
            "schema_version": 1,
            "operation": "locator.expect",
            "reason": "locator_no_match",
            "summary": "missing",
            "truncated": False,
        }
        malformed = [None, b'{"kind":"usage","message":"bytes"}', "{not-json", "[]", "{}"]
        malformed.extend(json.dumps(value) for value in (
            {"message": "missing kind"},
            {"kind": "usage"},
            {"kind": "unknown", "message": "bad kind"},
            {"kind": "usage", "message": None},
            {"kind": "assertion", "message": "bad details", "details": []},
            {"kind": "assertion", "message": "bad artifact", "artifact": []},
            {
                "kind": "assertion", "message": "bad reason",
                "details": {**valid_details, "reason": "unknown"},
            },
            {
                "kind": "assertion", "message": "bad locator",
                "details": {**valid_details, "locator": {"selectors": []}},
            },
        ))
        for artifact in (
            {},
            {"directory": "artifacts"},
            {"status": "written"},
            {"status": "unknown", "directory": "artifacts"},
            {"status": "written", "directory": 123},
            {"status": "written", "directory": "artifacts", "errors": "error"},
            {"status": "written", "directory": "artifacts", "errors": [None]},
            {"status": "written", "directory": "artifacts", "screen_svg": 123},
        ):
            malformed.append(json.dumps({
                "kind": "assertion", "message": "bad artifact", "artifact": artifact,
            }))
        for raw in malformed:
            with self.subTest(raw=raw):
                native_error = client.native.NativeAssertionError("native message")
                if raw is not None:
                    native_error._tui_test_error_json = raw

                async def fail():
                    raise native_error

                with self.assertRaises(InternalError) as raised:
                    run(client._await_native(fail()))
                self.assertIn("malformed native error envelope", raised.exception.message)
                self.assertIs(raised.exception.__cause__, native_error)

    def test_public_errors_expose_only_current_diagnostic_fields(self):
        self.assertFalse(hasattr(tui_test, "TerminalArtifact"))
        self.assertNotIn("TerminalArtifact", tui_test.__all__)
        error = ExpectationError("missing")
        self.assertFalse(hasattr(error, "terminal"))
        self.assertIsNone(error.details)
        self.assertIsNone(error.artifact)


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
        self.assertEqual(args[10:], (100, None, None, None, 200, None))

    def test_run_uses_program_and_argv(self):
        terminal = _CapturingClient("s")
        run(terminal.run("vim", "file.txt"))
        name, args = terminal.fake.calls[0]
        self.assertEqual(name, "run")
        self.assertEqual(args[0], "vim")
        self.assertEqual(args[1], ["file.txt"])

    def test_restart_forwards_timeout_and_preserves_result(self):
        terminal = _CapturingClient("s")
        terminal.fake.reply = {
            "shell_pid": 42,
            "session": "s",
            "ready": True,
            "recording": "",
        }
        result = run(terminal.restart(graceful_timeout=123))
        self.assertEqual(terminal.fake.calls, [("restart", (123,))])
        self.assertIs(result, terminal.fake.reply)

    def test_restart_preserves_native_errors(self):
        terminal = _CapturingClient("s")
        error = RuntimeError("restart failed")
        terminal.fake.error = error
        with self.assertRaises(RuntimeError) as raised:
            run(terminal.restart())
        self.assertIs(raised.exception, error)
        self.assertEqual(terminal.fake.calls, [("restart", (5000,))])

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

    def test_constructor_screen_history_limit_reaches_open_and_run(self):
        terminal = _CapturingClient("s", screen_history_limit=17)
        run(terminal.open())
        run(terminal.run("vim"))
        self.assertEqual(terminal.fake.calls[0][1][-1], 17)
        self.assertEqual(terminal.fake.calls[1][1][-1], 17)

    def test_obsolete_native_constructor_is_not_adapted(self):
        class ObsoleteNativeSession:
            def __init__(self, name, recording_mode, recording_directory):
                raise AssertionError("obsolete constructor must not be called")

        with mock.patch.object(
            client.native, "NativeSession", ObsoleteNativeSession
        ):
            with self.assertRaises(TypeError):
                client.TuiTest("s", screen_history_limit=17)

    def test_constructor_type_error_is_not_retried(self):
        error = TypeError("current argument is invalid")
        with mock.patch.object(
            client.native, "NativeSession", side_effect=error
        ) as constructor:
            with self.assertRaises(TypeError) as raised:
                client.TuiTest("s")
        self.assertIs(raised.exception, error)
        constructor.assert_called_once_with("s", None, None, None, False, None, None)

    def test_close_always_uses_current_outcome_argument(self):
        terminal = _CapturingClient("s")
        run(terminal.close())
        run(terminal.close(failed=False))
        run(terminal.close(failed=True))
        self.assertEqual(
            terminal.fake.calls,
            [("close", (None,)), ("close", (False,)), ("close", (True,))],
        )

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
        with self.assertRaisesRegex(ValueError, "unknown mouse button"):
            run(terminal.get_by_text("Open").click(button="primary"))

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
                    ("demo.png", "apng", 24, 2.0, 3.0, 0.5, None, False),
                ),
                ("stop_recording", ()),
            ],
        )

    def test_screenshot_forwards_zoom(self):
        terminal = _CapturingClient("s")
        run(terminal.screenshot("screen.svg", full=True, zoom=0.5))
        self.assertEqual(
            terminal.fake.calls,
            [("screenshot", ("screen.svg", True, 0.5, None, False))],
        )

    def test_screenshot_rejects_zoom_without_path(self):
        terminal = _CapturingClient("s")
        with self.assertRaisesRegex(ValueError, "requires a path"):
            run(terminal.screenshot(zoom=0.5))

    def test_capture_background_options_are_forwarded(self):
        terminal = _CapturingClient("s")
        run(terminal.screenshot("screen.svg", background="#123456"))
        run(terminal.start_recording("demo.gif", transparent=True))
        self.assertEqual(
            terminal.fake.calls,
            [
                ("screenshot", ("screen.svg", False, None, "#123456", False)),
                (
                    "start_recording",
                    ("demo.gif", None, None, None, None, None, None, True),
                ),
            ],
        )


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
        run(terminal.get_by_text("x").wait())
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
            terminal.fake.calls[0][1][-6:-1],
            (1000, None, None, None, 2000),
        )
        self.assertEqual(
            terminal.fake.calls[1][1][-6:-1],
            (None, 1500, None, None, None),
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
            "wait_idle": (),
            "wait_command": (),
            "wait_exit": (),
            "wait_ready": (),
            "expect_exit_code": (0,),
            "expect_output": ("x",),
            "expect_snapshot": ("x",),
        }
        for name, args in cases.items():
            self.assertTrue(
                self._prefix_for(name, *args).startswith(name + ": ")
            )

        terminal = _CapturingClient("s")
        terminal.fake.error = ExpectationError("boom")
        for name, operation in (
            ("locator.wait", terminal.get_by_text("x").wait),
            ("locator.expect", terminal.get_by_text("x").expect),
        ):
            with self.assertRaises(ExpectationError) as raised:
                run(operation())
            self.assertTrue(str(raised.exception).startswith(name + ": "))


class LocatorTests(unittest.TestCase):
    @staticmethod
    def _match(text="Save", row=0, column=0):
        return {
            "text": text,
            "start": {"row": row, "column": column},
            "end": {"row": row, "column": column + len(text)},
            "spans": [
                {
                    "row": row,
                    "start": column,
                    "end": column + len(text),
                }
            ],
        }

    def test_selector_and_style_options_use_typed_native_methods(self):
        terminal = _CapturingClient("s")
        terminal.fake.reply = []
        run(
            terminal.get_by_text("Settings")
            .get_by_text(
                "Save",
                whitespace="normalize",
                direction="after",
            )
            .nth(1)
            .locations()
        )
        name, args = terminal.fake.calls[0]
        self.assertEqual(name, "find_locator")
        stages = args[0]["nodes"]
        self.assertEqual(stages[0]["text"], "Settings")
        selector = stages[-1]
        self.assertEqual(selector["direction"], "after")
        self.assertEqual(selector["occurrence"], "nth")
        self.assertEqual(selector["nth"], 1)

        run(
            terminal.get_by_text("Warning")
            .get_by_style(
                TextStyle(bold=True, underline_style="curly")
            )
            .first()
            .expect()
        )
        name, args = terminal.fake.calls[1]
        self.assertEqual(name, "expect_locator")
        query, not_ = args[:2]
        query = query["nodes"]
        style = query[-1]["style"]
        self.assertEqual(query[-1]["direction"], "within")
        self.assertEqual(query[-1]["occurrence"], "first")
        self.assertTrue(style["bold"])
        self.assertEqual(style["underline_style"], "curly")
        self.assertFalse(not_)

    def test_locator_selection_is_lazy_and_immutable(self):
        terminal = _CapturingClient("s")
        terminal.fake.reply = [self._match(), self._match(row=1)]
        locator = terminal.get_by_text("Save", whitespace="normalize")

        self.assertEqual(run(locator.count()), 2)
        items = run(locator.all())
        self.assertEqual(len(items), 2)
        self.assertIsInstance(items[0], client.Locator)

        terminal.fake.reply = [self._match(row=1)]
        match = run(items[1].location())
        self.assertEqual(match.start.row, 1)
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "find_locator")
        query = args[0]["nodes"]
        self.assertTrue(args[1])
        self.assertEqual(query[-1]["occurrence"], "nth")
        self.assertEqual(query[-1]["nth"], 1)

        run(locator.first().locations())
        selected = terminal.fake.calls[-1][1][0]["nodes"]
        self.assertEqual(selected[-1]["occurrence"], "first")
        run(locator.locations())
        original = terminal.fake.calls[-1][1][0]["nodes"]
        self.assertEqual(original[-1]["occurrence"], "any")

        run(
            terminal.get_by_text("Save Save")
            .get_by_text("Save")
            .get_by_text("av")
            .locations()
        )
        nested = terminal.fake.calls[-1][1][0]["nodes"]
        self.assertEqual([stage["text"] for stage in nested], [
            "Save Save",
            "Save",
            "av",
        ])

        run(
            terminal.get_by_style(TextStyle(bold=True))
            .get_by_text("Save")
            .locations()
        )
        styled = terminal.fake.calls[-1][1][0]["nodes"]
        self.assertEqual(styled[-1]["kind"], "text")
        self.assertEqual(styled[-2]["kind"], "style")
        self.assertTrue(styled[-2]["style"]["bold"])

    def test_locator_actions_use_selector_aware_native_operations(self):
        terminal = _CapturingClient("s", timeouts=Timeouts(text=1234))
        terminal.fake.reply = [self._match()]
        locator = terminal.get_by_text("Save")

        waited = run(locator.wait())
        self.assertIs(waited, locator)
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "wait_locator")
        self.assertFalse(args[1])
        self.assertEqual(args[2], 1234)

        run(
            locator.click(
                button="middle",
                alt=True,
                ctrl=True,
                shift=True,
                clicks=2,
                timeout=50,
            )
        )
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "click_locator")
        self.assertEqual(args[0]["nodes"][args[0]["root"]]["occurrence"], "any")
        self.assertEqual(args[1:], (29, 2, 50))

        run(locator.highlight())
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "highlight_locator")
        self.assertEqual(args[0]["nodes"][args[0]["root"]]["occurrence"], "any")

        run(locator.expect())
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "expect_locator")
        query, not_ = args[:2]
        query = query["nodes"]
        self.assertEqual(query[-1]["occurrence"], "any")
        self.assertFalse(not_)

        run(locator.unique().expect())
        query, not_ = terminal.fake.calls[-1][1][:2]
        query = query["nodes"]
        self.assertEqual(query[-1]["occurrence"], "unique")
        self.assertFalse(not_)

    def test_relative_locator_wait_returns_the_same_locator(self):
        terminal = _CapturingClient("s")
        locator = (
            terminal.get_by_text("Settings")
            .get_by_text(
                "Save",
                whitespace="normalize",
                direction="after",
            )
        )
        self.assertIs(run(locator.wait()), locator)
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "wait_locator")
        query = args[0]["nodes"]
        self.assertEqual(query[-1]["direction"], "after")

    def test_locator_rejects_invalid_selection_and_state(self):
        terminal = _CapturingClient("s")
        locator = terminal.get_by_text("Save")
        with self.assertRaisesRegex(ValueError, "non-negative integer"):
            locator.nth(-1)
        with self.assertRaisesRegex(ValueError, "locator state"):
            run(locator.wait(state="gone"))
        with self.assertRaisesRegex(ValueError, "locator direction"):
            locator.get_by_text("child", direction="sideways")
        with self.assertRaises(TypeError):
            terminal.get_by_text("Save", occurrence="last")
        with self.assertRaises(TypeError):
            locator.expect(style=TextStyle(bold=True))
        with self.assertRaisesRegex(ValueError, "at least one style"):
            terminal.get_by_style(TextStyle())

    def test_composition_is_lazy_immutable_and_owner_checked(self):
        terminal = _CapturingClient("s")
        terminal.fake.reply = []
        bold = terminal.get_by_style(TextStyle(bold=True))
        link = terminal.get_by_link("test:link")
        composed = bold.and_(link).or_(terminal.get_by_text("fallback"))
        filtered = composed.filter(has=link, has_not=terminal.get_by_text("old"))
        self.assertEqual(terminal.fake.calls, [])
        run(filtered.nth(1).locations())
        expression = terminal.fake.calls[-1][1][0]
        root = expression["nodes"][expression["root"]]
        self.assertEqual(root["kind"], "filter")
        self.assertEqual(root["nth"], 1)
        self.assertEqual(expression["nodes"][root["input"]]["kind"], "or")
        self.assertEqual(expression["nodes"][root["has"]]["link"], "test:link")
        run(bold.locations())
        self.assertEqual(len(terminal.fake.calls[-1][1][0]["nodes"]), 1)
        other = _CapturingClient("s").get_by_link("test:link")
        for operation in (lambda: bold.and_(other), lambda: bold.or_(other), lambda: bold.filter(has=other)):
            with self.assertRaisesRegex(ValueError, "same terminal owner"):
                operation()
        with self.assertRaises(ValueError):
            bold.filter()
        with self.assertRaises(TypeError):
            bold.filter(has=link, link="test:link")
        with self.assertRaises(TypeError):
            TextStyle(link="test:link")

    def test_location_rejects_an_invalid_native_match_count(self):
        terminal = _CapturingClient("s")
        terminal.fake.reply = []
        with self.assertRaises(InternalError) as raised:
            run(terminal.get_by_text("missing").location())
        self.assertIn("invalid match count", str(raised.exception))
        name, args = terminal.fake.calls[-1]
        self.assertEqual(name, "find_locator")
        self.assertIn("nodes", args[0])
        self.assertTrue(args[1])


class ContextManagerTests(unittest.TestCase):
    def test_close_receives_the_final_outcome(self):
        for failure in (None, RuntimeError("test failed"), asyncio.CancelledError()):
            with self.subTest(failure=type(failure).__name__):
                terminal = _CapturingClient("s")

                async def body():
                    async with terminal:
                        if failure is not None:
                            raise failure

                async def scenario():
                    if failure is None:
                        await body()
                    else:
                        with self.assertRaises(type(failure)) as raised:
                            await body()
                        self.assertIs(raised.exception, failure)

                run(scenario())
                self.assertEqual(
                    terminal.fake.calls, [("close", (failure is not None,))]
                )

    def test_cleanup_error_does_not_mask_body_failure_or_cancellation(self):
        for failure in (RuntimeError("test failed"), asyncio.CancelledError()):
            with self.subTest(failure=type(failure).__name__):
                terminal = _CapturingClient("s")
                terminal.fake.error = InternalError("trace export failed")

                async def scenario():
                    with self.assertRaises(type(failure)) as raised:
                        async with terminal:
                            raise failure
                    self.assertIs(raised.exception, failure)

                run(scenario())
                self.assertEqual(terminal.fake.calls, [("close", (True,))])

    def test_cleanup_error_is_reported_when_body_succeeds(self):
        terminal = _CapturingClient("s")
        terminal.fake.error = InternalError("trace export failed")

        async def scenario():
            async with terminal:
                pass

        with self.assertRaises(InternalError) as raised:
            run(scenario())
        self.assertIs(raised.exception, terminal.fake.error)
        self.assertEqual(terminal.fake.calls, [("close", (False,))])


class ArtifactCaptureTests(unittest.TestCase):
    def test_bytes_artifact_directory_is_decoded_before_native_constructor(self):
        class BytesPath:
            def __fspath__(self):
                return os.fsencode(os.path.join("relative", "artifacts-é"))

        for directory in (BytesPath(), os.fspath(BytesPath())):
            with self.subTest(directory=directory), mock.patch.object(
                client.native, "NativeSession", wraps=client.native.NativeSession
            ) as constructor:
                terminal = client.TuiTest(
                    "s", artifacts={"dir": directory, "on_failure": "text"}
                )
                self.assertEqual(
                    constructor.call_args.args[2],
                    os.path.abspath(os.path.join("relative", "artifacts-é")),
                )
                with self.assertRaisesRegex(UsageError, "cols"):
                    run(terminal.open(cols=-1))

    def test_artifact_options_are_absolute_and_passed_native(self):
        native_session = mock.Mock()
        with mock.patch.object(
            client.native, "NativeSession", return_value=native_session
        ) as constructor:
            client.TuiTest(
                "s",
                artifacts={
                    "dir": os.path.join("relative", "artifacts"),
                    "on_failure": "all",
                    "include_recording": True,
                },
            )
        args = constructor.call_args.args
        self.assertEqual(
            args[2], os.path.abspath(os.path.join("relative", "artifacts"))
        )
        self.assertEqual(args[3], "all")
        self.assertTrue(args[4])

    def test_none_artifact_mode_does_not_require_directory(self):
        with mock.patch.object(client.native, "NativeSession") as constructor:
            client.TuiTest("s", artifacts={"on_failure": "none"})
        self.assertIsNone(constructor.call_args.args[2])
        self.assertEqual(constructor.call_args.args[3], "none")

    def test_all_failure_artifact_modes_are_mapped(self):
        for mode in ("all", "html", "text"):
            with self.subTest(mode=mode), mock.patch.object(
                client.native, "NativeSession"
            ) as constructor:
                client.TuiTest(
                    "s",
                    artifacts={"dir": "artifacts", "on_failure": mode},
                )
            self.assertEqual(constructor.call_args.args[3], mode)

    def test_invalid_artifact_modes_are_rejected_and_default_is_all(self):
        with mock.patch.object(client.native, "NativeSession") as constructor:
            client.TuiTest("s", artifacts={"dir": "artifacts"})
        self.assertEqual(constructor.call_args.args[3], "all")
        for mode in ("bundle", "svg", "json"):
            with self.subTest(mode=mode), self.assertRaisesRegex(ValueError, "all, html, text, or none"):
                client.TuiTest("s", artifacts={"dir": "artifacts", "on_failure": mode})

    def test_native_artifact_references_are_not_recaptured_or_read(self):
        terminal = _CapturingClient(
            "s", artifacts={"dir": "unused", "on_failure": "all"}
        )
        native_error = client.native.NativeAssertionError("native message")
        native_error._tui_test_error_json = json.dumps({
            "kind": "assertion",
            "message": "structured",
            "details": {
                "schema_version": 1,
                "operation": "locator.wait",
                "reason": "locator_no_match",
                "summary": "missing",
                "truncated": False,
            },
            "artifact": {
                "status": "partial",
                "directory": "core-artifacts",
                "screen_text": "core-artifacts/current.txt",
                "screen_svg": "core-artifacts/current.svg",
                "errors": ["recording omitted"],
            },
        })
        terminal.fake.error = native_error
        terminal.screenshot = mock.AsyncMock(side_effect=AssertionError("unexpected capture"))
        with mock.patch.object(
            client, "open", create=True, side_effect=AssertionError("unexpected file read")
        ) as open_file:
            with self.assertRaises(ExpectationError) as raised:
                run(terminal.get_by_text("x").wait())
        error = raised.exception
        self.assertEqual(error.message, "locator.wait: structured")
        self.assertIs(error.__cause__, native_error)
        self.assertEqual(error.details.reason, FailureReason.LOCATOR_NO_MATCH)
        self.assertEqual(error.artifact.status, FailureArtifactStatus.PARTIAL)
        self.assertEqual(error.artifact.screen_text, "core-artifacts/current.txt")
        self.assertEqual(error.artifact.screen_svg, "core-artifacts/current.svg")
        self.assertEqual(error.artifact.errors, ("recording omitted",))
        self.assertFalse(hasattr(error, "terminal"))
        terminal.screenshot.assert_not_awaited()
        open_file.assert_not_called()


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
