import asyncio
import inspect
import unittest
from pathlib import Path

from shell_use import _native
from shell_use import unique_session


class _IndexValue:
    def __init__(self, value):
        self.value = value

    def __index__(self):
        return self.value


class NativeSurfaceTests(unittest.TestCase):
    def test_native_session_has_only_typed_terminal_methods(self):
        session = _native.NativeSession(unique_session("surface"))
        self.assertFalse(hasattr(session, "request"))
        for name in (
            "open",
            "run",
            "close",
            "state",
            "text",
            "packed_screen",
            "cells",
            "get_command",
            "get_output",
            "get_exit_code",
            "get_cwd",
            "get_cursor",
            "get_size",
            "write",
            "type",
            "submit",
            "press",
            "keys",
            "mouse_click",
            "mouse_move",
            "mouse_down",
            "mouse_up",
            "mouse_drag",
            "mouse_scroll",
            "resize",
            "signal",
            "kill",
            "wait_text",
            "wait_idle",
            "wait_command",
            "wait_exit",
            "wait_ready",
            "expect_text",
            "expect_exit_code",
            "expect_output",
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


class NativeStubTests(unittest.TestCase):
    def test_native_futures_are_annotated_as_awaitables(self):
        stub = (
            Path(__file__).resolve().parents[1]
            / "src"
            / "shell_use"
            / "_native.pyi"
        ).read_text(encoding="utf-8")
        self.assertNotIn("async def ", stub)
        self.assertIn("def open(", stub)
        self.assertIn("typing.Awaitable[", stub)


if __name__ == "__main__":
    unittest.main()
