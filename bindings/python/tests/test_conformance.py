import json
import os
import subprocess
import unittest

import shell_use
from shell_use import ShellUse

BIN = os.environ.get("SHELL_USE_BIN") or "shell-use"

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
    ],
    "type": [("client", "type")],
    "submit": [("client", "submit")],
    "press": [("client", "press")],
    "keys": [("client", "keys")],
    "mouse": [("client", "mouse")],
    "resize": [("client", "resize")],
    "write": [("client", "write")],
    "signal": [("client", "signal")],
    "kill": [("client", "kill")],
    "wait": [("client", "wait_title"), ("client", "wait_text"), ("client", "wait_idle"), ("client", "wait_command"), ("client", "wait_exit")],
    "expect": [("client", "expect_title"), ("client", "expect_text"), ("client", "expect_exit_code"), ("client", "expect_output"), ("client", "expect_snapshot")],
    "get-recording": [("module", "get_recording")],
}

EXCLUDED = {"monitor", "usage", "agent-context", "skill", "daemon"}


def _have_binary():
    try:
        subprocess.run([BIN, "agent-context"], capture_output=True, check=True)
        return True
    except Exception:
        return False


@unittest.skipUnless(_have_binary(), "shell-use binary not available for agent-context")
class ConformanceTests(unittest.TestCase):
    def test_every_command_is_mapped_or_excluded(self):
        out = subprocess.run([BIN, "agent-context"], capture_output=True, check=True, text=True).stdout
        schema = json.loads(out)
        commands = set(schema["commands"].keys())

        for command in commands:
            if command in EXCLUDED:
                continue
            self.assertIn(command, MAPPING, f"cli command '{command}' has no SDK mapping")
            instance = ShellUse("conformance")
            for scope, attr in MAPPING[command]:
                target = instance if scope == "client" else shell_use
                self.assertTrue(
                    hasattr(target, attr),
                    f"missing SDK member for '{command}': {scope}.{attr}",
                )

    def test_exit_codes_match(self):
        out = subprocess.run([BIN, "agent-context"], capture_output=True, check=True, text=True).stdout
        schema = json.loads(out)
        codes = schema["exit_codes"]
        self.assertEqual(shell_use.ExpectationError.exit_code, 1)
        self.assertEqual(shell_use.UsageError.exit_code, 2)
        self.assertEqual(shell_use.NoSessionError.exit_code, 3)
        self.assertEqual(shell_use.InternalError.exit_code, 5)
        self.assertIn("1", codes)
        self.assertIn("3", codes)


if __name__ == "__main__":
    unittest.main()
