import asyncio
import inspect
import sys
import time
import unittest
from pathlib import Path

from tui_test import (
    ExpectationError,
    Locator,
    NoSessionError,
    Timeouts,
    TuiTest,
    TuiTestError,
    _native,
    unique_session,
)


class _IndexValue:
    def __init__(self, value):
        self.value = value

    def __index__(self):
        return self.value


class NativeSurfaceTests(unittest.TestCase):
    def test_public_client_omits_one_shot_text_methods(self):
        for name in ("find_text", "wait_text", "expect_text"):
            self.assertFalse(hasattr(TuiTest, name), name)
        for method in (
            TuiTest.get_by_text,
            TuiTest.get_by_style,
            Locator.get_by_text,
            Locator.get_by_style,
        ):
            self.assertNotIn("occurrence", inspect.signature(method).parameters)
        self.assertNotIn(
            "style", inspect.signature(Locator.expect).parameters
        )

    def test_native_session_has_only_typed_terminal_methods(self):
        session = _native.NativeSession(unique_session("surface"))
        self.assertFalse(hasattr(session, "request"))
        for name in (
            "open",
            "run",
            "close",
            "begin_monitor_wait",
            "wait_for_monitor",
            "close_monitor_target",
            "cancel_monitor_wait",
            "state",
            "text",
            "find_locator",
            "wait_locator",
            "click_locator",
            "highlight_locator",
            "expect_locator",
            "packed_screen",
            "cells",
            "get_command",
            "get_output",
            "get_exit_code",
            "get_cwd",
            "get_cursor",
            "get_size",
            "get_clipboard",
            "get_bell_count",
            "get_bell_events",
            "write",
            "type",
            "submit",
            "press",
            "key_down",
            "repeat",
            "key_up",
            "mouse_click",
            "mouse_move",
            "mouse_down",
            "mouse_up",
            "mouse_drag",
            "mouse_scroll",
            "resize",
            "signal",
            "kill",
            "wait_clipboard",
            "wait_idle",
            "wait_command",
            "wait_exit",
            "wait_ready",
            "wait_bell",
            "expect_exit_code",
            "expect_output",
            "expect_bell_count",
            "snapshot",
            "screenshot",
            "start_recording",
            "stop_recording",
            "recording",
        ):
            self.assertTrue(hasattr(session, name), name)

    def test_native_error_classes_are_distinct(self):
        classes = {
            _native.NativeAssertionError,
            _native.NativeUsageError,
            _native.NativeNoSessionError,
            _native.NativeInternalError,
        }
        self.assertEqual(len(classes), 4)
        for exception in classes:
            self.assertTrue(issubclass(exception, Exception))

    def test_invalid_integer_is_reported_from_native_awaitable(self):
        async def scenario():
            session = _native.NativeSession(unique_session("native-number"))
            awaitable = session.resize(-1, 24)
            self.assertTrue(inspect.isawaitable(awaitable))
            with self.assertRaises(_native.NativeUsageError):
                await awaitable

        asyncio.run(scenario())

    def test_locator_stages_reject_cross_kind_fields(self):
        async def scenario():
            session = _native.NativeSession(unique_session("native-locator"))
            for stage in (
                {"kind": "text", "text": "x", "style": {"bold": True}},
                {"kind": "style", "style": {"bold": True}, "text": "x"},
            ):
                with self.assertRaises(_native.NativeUsageError):
                    await session.find_locator([stage])

        asyncio.run(scenario())

    def test_index_objects_are_accepted_before_range_validation(self):
        async def scenario():
            session = _native.NativeSession(unique_session("native-index"))
            with self.assertRaises(_native.NativeNoSessionError):
                await session.resize(_IndexValue(80), _IndexValue(24))

        asyncio.run(scenario())

    def test_unsigned_values_above_i64_are_accepted(self):
        async def scenario():
            session = _native.NativeSession(unique_session("native-u64"))
            with self.assertRaises(_native.NativeNoSessionError):
                await session.wait_idle(2**63)

        asyncio.run(scenario())

    def test_monitoring_metadata_json_is_strict(self):
        _native.NativeSession(
            unique_session("native-metadata"),
            monitoring_metadata='{"label":"test","test_file":"test_file.py"}',
        )
        for value in (
            "invalid", "null", "[]", '{"label":1}',
            '{"testFile":"test.py"}', '{"worker":null}',
        ):
            with self.subTest(value=value), self.assertRaises(_native.NativeUsageError):
                _native.NativeSession(
                    unique_session("invalid-metadata"), monitoring_metadata=value,
                )

    def test_monitor_wait_requires_an_opened_target(self):
        session = _native.NativeSession(unique_session("monitor-no-session"))
        with self.assertRaises(_native.NativeNoSessionError):
            session.begin_monitor_wait("failed", 30_000, True)
        with self.assertRaises(_native.NativeUsageError):
            session.begin_monitor_wait("unknown", 30_000, True)

    def test_monitor_timeout_rejects_bool_from_native_awaitable(self):
        async def scenario():
            session = _native.NativeSession(unique_session("monitor-timeout"))
            for value in (True, -1, 1.5):
                with self.subTest(value=value), self.assertRaises(_native.NativeUsageError):
                    await session.wait_for_monitor(1, value, True)

        asyncio.run(scenario())

    def test_monitor_without_client_times_out_off_event_loop(self):
        async def scenario():
            terminal = TuiTest.ephemeral(
                "monitor-finite", recording={"mode": "disabled"},
                monitoring={
                    "enabled": True,
                    "wait_at_end": "always",
                    "first_attach_timeout": 100,
                },
            )
            try:
                await terminal.run(
                    sys.executable, "-c", "import time; time.sleep(30)",
                    wait_ready=False,
                )
                ticks = []

                async def tick():
                    await asyncio.sleep(0.01)
                    ticks.append(time.monotonic())

                timer = asyncio.create_task(tick())
                start = time.monotonic()
                await asyncio.wait_for(terminal.finish(), 5)
                end = time.monotonic()
                await timer
                self.assertLess(ticks[0], end)
                self.assertGreaterEqual(end - start, 0.08)
            finally:
                await terminal.close_quiet()

        asyncio.run(scenario())

    def test_stale_client_cannot_mark_or_close_monitored_replacement(self):
        async def scenario():
            for operation in ("close", "finish", "cancel"):
                name = unique_session("monitor-replacement")
                original = TuiTest(
                    name, recording={"mode": "disabled"},
                    monitoring={
                        "enabled": True, "wait_at_end": "failure",
                        "first_attach_timeout": 0,
                    },
                )
                replacement = TuiTest(
                    name, recording={"mode": "disabled"},
                    monitoring={
                        "enabled": True, "wait_at_end": "never",
                        "first_attach_timeout": 0,
                    },
                )
                try:
                    await original.run(
                        sys.executable, "-c", "import time; time.sleep(30)",
                        wait_ready=False,
                    )
                    await replacement.run(
                        sys.executable, "-c", "import time; time.sleep(30)",
                        wait_ready=False, restart=True, cols=73, rows=19,
                    )
                    with self.subTest(operation=operation):
                        if operation == "close":
                            await original.close()
                        elif operation == "finish":
                            with self.assertRaises(NoSessionError):
                                await original.finish("failed")
                        else:
                            original._native.cancel_monitor_wait()
                            _, generation, _ = replacement._native.begin_monitor_wait(
                                "passed", 0, False
                            )
                            await replacement._native.wait_for_monitor(
                                generation, 0, False
                            )
                        self.assertEqual(
                            await replacement.get_size(), {"cols": 73, "rows": 19}
                        )
                finally:
                    await original.close_quiet()
                    await replacement.close_quiet()

        asyncio.run(scenario())

    def test_pre_wait_cancellation_does_not_start_inspection_or_destroy_child(self):
        async def scenario():
            terminal = TuiTest.ephemeral(
                "monitor-pre-wait-cancel", recording={"mode": "disabled"},
                monitoring={"enabled": True, "wait_at_end": "never"},
            )
            try:
                await terminal.run(
                    sys.executable, "-c", "import time; time.sleep(30)",
                    wait_ready=False,
                )
                terminal._native.cancel_monitor_wait()
                self.assertIsNone((await terminal.state()).exited)
                with self.assertRaises(_native.NativeUsageError):
                    terminal._native.begin_monitor_wait("failed", None, True)
                await asyncio.wait_for(terminal.close(), 5)
            finally:
                await asyncio.wait_for(terminal.close_quiet(), 5)

        asyncio.run(scenario())

    def test_cancelling_native_wait_releases_the_generation_hold(self):
        async def scenario():
            terminal = TuiTest.ephemeral(
                "monitor-cancel", recording={"mode": "disabled"},
                monitoring={"enabled": True, "wait_at_end": "never"},
            )
            generation = None
            waiting = None
            try:
                await terminal.run(
                    sys.executable, "-c", "import time; time.sleep(30)",
                    wait_ready=False,
                )
                _, generation, _ = terminal._native.begin_monitor_wait(
                    "failed", None, True
                )
                waiting = asyncio.ensure_future(
                    terminal._native.wait_for_monitor(generation, None, True)
                )
                await asyncio.sleep(0.05)
                waiting.cancel()
                with self.assertRaises(asyncio.CancelledError):
                    await waiting
                await asyncio.wait_for(terminal.close(), 5)
            finally:
                if waiting is not None and not waiting.done():
                    waiting.cancel()
                if generation is not None:
                    terminal._native.cancel_monitor_wait(generation)
                await asyncio.wait_for(terminal.close_quiet(), 5)

        asyncio.run(scenario())

    def test_monitored_readiness_failure_preserves_child_and_original_error(self):
        async def scenario():
            terminal = TuiTest.ephemeral(
                "monitor-readiness", recording={"mode": "disabled"},
                monitoring={
                    "enabled": True, "wait_at_end": "failure",
                    "first_attach_timeout": 0,
                },
            )
            try:
                try:
                    await terminal.run(
                        sys.executable, "-u", "-c",
                        "import time; print('READINESS_FAILURE_CHILD_LIVE'); time.sleep(30)",
                        wait_ready=True, timeouts=Timeouts(ready=50),
                    )
                except ExpectationError as original:
                    original_traceback = original.__traceback__
                    self.assertIsNone((await terminal.state()).exited)
                    await terminal.get_by_text("READINESS_FAILURE_CHILD_LIVE").first().expect(
                        timeout=10_000
                    )
                    try:
                        await terminal.inspect_failure(original)
                    except ExpectationError as raised:
                        self.assertIs(raised, original)
                        traceback = raised.__traceback__
                        while traceback is not None and traceback is not original_traceback:
                            traceback = traceback.tb_next
                        self.assertIs(traceback, original_traceback)
                    else:
                        self.fail("readiness error was swallowed after inspection")
                else:
                    self.fail("non-shell child unexpectedly reported shell readiness")
            finally:
                await terminal.close_quiet()

        asyncio.run(scenario())

    def test_unspawnable_client_cleanup_cannot_close_later_replacement(self):
        async def scenario():
            name = unique_session("monitor-unspawnable")
            failed = TuiTest(
                name, recording={"mode": "disabled"},
                monitoring={
                    "enabled": True, "wait_at_end": "failure",
                    "first_attach_timeout": 0,
                },
            )
            replacement = TuiTest(
                name, recording={"mode": "disabled"},
                monitoring={"enabled": False},
            )
            try:
                try:
                    await failed.run(sys.executable + ".does-not-exist", wait_ready=False)
                except TuiTestError as original:
                    await replacement.run(
                        sys.executable, "-c", "import time; time.sleep(30)",
                        wait_ready=False, cols=69, rows=18,
                    )
                    with self.assertRaises(TuiTestError) as raised:
                        await failed.inspect_failure(original)
                    self.assertIs(raised.exception, original)
                    self.assertEqual(
                        await replacement.get_size(), {"cols": 69, "rows": 18}
                    )
                else:
                    self.fail("nonexistent program unexpectedly started")
            finally:
                await failed.close_quiet()
                await replacement.close_quiet()

        asyncio.run(scenario())


class NativeStubTests(unittest.TestCase):
    def test_native_futures_are_annotated_as_awaitables(self):
        stub = (
            Path(__file__).resolve().parents[1]
            / "src"
            / "tui_test"
            / "_native.pyi"
        ).read_text(encoding="utf-8")
        self.assertNotIn("async def ", stub)
        self.assertNotIn("query_json", stub)
        self.assertNotIn("request_json", stub)
        self.assertIn("def open(", stub)
        self.assertIn(
            "def find_locator(self, stages: typing.List[typing.Dict[str, typing.Any]])",
            stub,
        )
        self.assertIn("typing.Awaitable[", stub)
        self.assertIn("monitoring_metadata: typing.Optional[str] = None", stub)
        self.assertIn("def begin_monitor_wait(", stub)
        self.assertIn("def wait_for_monitor(", stub)
        self.assertIn("def cancel_monitor_wait(", stub)
        self.assertIn("def close_monitor_target(", stub)


if __name__ == "__main__":
    unittest.main()
