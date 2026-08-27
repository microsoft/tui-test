import json
import os
import subprocess
import unittest

import tui_test
from tui_test import TuiTest

BIN = os.environ.get("TUI_TEST_BIN") or "tui-test"

MAPPING = {
    "open": [("client", "open")],
    "run": [("client", "run")],
    "close": [("client", "close"), ("module", "close_all")],
    "sessions": [("module", "sessions")],
    "state": [("client", "state")],
    "text": [("client", "text")],
    "screenshot": [("client", "screenshot")],
    "record": [("client", "start_recording"), ("client", "stop_recording")],
    "cells": [("client", "cells")],
    "get": [
        ("client", "get_command"),
        ("client", "get_output"),
        ("client", "get_exit_code"),
        ("client", "get_cwd"),
        ("client", "get_cursor"),
        ("client", "get_size"),
        ("client", "get_title"),
        ("client", "get_bell_count"),
        ("client", "get_bell_events"),
    ],
    "type": [("client", "type")],
    "submit": [("client", "submit")],
    "key": [("client", "keyboard")],
    "press": [("client", "press")],
    "mouse": [("client", "mouse")],
    "resize": [("client", "resize")],
    "write": [("client", "write")],
    "signal": [("client", "signal")],
    "kill": [("client", "kill")],
    "wait": [("client", "wait_title"), ("client", "wait_text"), ("client", "wait_idle"), ("client", "wait_command"), ("client", "wait_exit"), ("client", "wait_bell")],
    "expect": [("client", "expect_title"), ("client", "expect_text"), ("client", "expect_exit_code"), ("client", "expect_output"), ("client", "expect_bell_count"), ("client", "expect_snapshot")],
    "find": [("client", "find_text")],
    "locator": [("client", "locator")],
    "get-recording": [("module", "get_recording")],
}

EXCLUDED = {"monitor", "usage", "agent-context", "skill", "daemon"}


def _have_binary():
    try:
        subprocess.run([BIN, "agent-context"], capture_output=True, check=True)
        return True
    except Exception:
        return False


@unittest.skipUnless(_have_binary(), "tui-test binary not available for agent-context")
class ConformanceTests(unittest.TestCase):
    def test_every_command_is_mapped_or_excluded(self):
        out = subprocess.run([BIN, "agent-context"], capture_output=True, check=True, text=True).stdout
        schema = json.loads(out)
        commands = set(schema["commands"].keys())

        for command in commands:
            if command in EXCLUDED:
                continue
            self.assertIn(command, MAPPING, f"cli command '{command}' has no SDK mapping")
            instance = TuiTest("conformance")
            for scope, attr in MAPPING[command]:
                target = instance if scope == "client" else tui_test
                self.assertTrue(
                    hasattr(target, attr),
                    f"missing SDK member for '{command}': {scope}.{attr}",
                )

    def test_keyboard_exposes_every_key_action(self):
        keyboard = TuiTest("keyboard-conformance").keyboard
        for method in ("press", "down", "repeat", "up"):
            self.assertTrue(hasattr(keyboard, method), method)

    def test_exit_codes_match(self):
        out = subprocess.run([BIN, "agent-context"], capture_output=True, check=True, text=True).stdout
        schema = json.loads(out)
        codes = schema["exit_codes"]
        self.assertEqual(tui_test.ExpectationError.exit_code, 1)
        self.assertEqual(tui_test.UsageError.exit_code, 2)
        self.assertEqual(tui_test.NoSessionError.exit_code, 3)
        self.assertEqual(tui_test.InternalError.exit_code, 5)
        self.assertIn("1", codes)
        self.assertIn("3", codes)


if __name__ == "__main__":
    unittest.main()
