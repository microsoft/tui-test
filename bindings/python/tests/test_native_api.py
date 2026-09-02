import asyncio
import inspect
import json
import unittest
from pathlib import Path

from tui_test import Locator, TuiTest, _native, unique_session


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
            with self.assertRaises(_native.NativeUsageError) as raised:
                await awaitable
            envelope = json.loads(
                raised.exception._tui_test_error_json
            )
            self.assertEqual(envelope["kind"], "usage")
            self.assertIn("cols", envelope["message"])
            self.assertFalse(
                hasattr(
                    _native.NativeUsageError,
                    "_tui_test_error_json",
                )
            )

        asyncio.run(scenario())

    def test_locator_stages_reject_cross_kind_fields(self):
        async def scenario():
            session = _native.NativeSession(unique_session("native-locator"))
            for stage in (
                {"kind": "text", "text": "x", "style": {"bold": True}},
                {"kind": "style", "style": {"bold": True}, "text": "x"},
            ):
                with self.assertRaises(_native.NativeUsageError):
                    await session.find_locator([stage], False)

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

    def test_screen_history_limit_is_validated_by_core(self):
        async def scenario():
            session = _native.NativeSession(unique_session("native-history"))
            with self.assertRaises(_native.NativeUsageError) as raised:
                await session.open(
                    None,
                    None,
                    80,
                    24,
                    None,
                    [],
                    None,
                    False,
                    None,
                    [],
                    None,
                    None,
                    None,
                    None,
                    None,
                    51,
                )
            self.assertIn("at most 50", str(raised.exception))

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
            "def find_locator(self, stages: typing.List[typing.Dict[str, typing.Any]], require_one: bool)",
            stub,
        )
        self.assertIn("typing.Awaitable[", stub)


if __name__ == "__main__":
    unittest.main()
