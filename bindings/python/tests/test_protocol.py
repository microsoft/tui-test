import asyncio
import unittest

from shell_use import client
from shell_use._protocol import env_pairs, unwrap
from shell_use.errors import (
    ExpectationError,
    InternalError,
    NoSessionError,
    UsageError,
)


def run(coro):
    return asyncio.run(coro)


class CapturingClient(client.ShellUse):
    def __init__(self, *a, **k):
        super().__init__(*a, **k)
        self.sent = []
        self.reply = {"ok": True, "data": {}}

    async def send(self, payload):
        self.sent.append(payload)
        return unwrap(self.reply)


class ProtocolTests(unittest.TestCase):
    def test_env_pairs_from_mapping(self):
        self.assertEqual(env_pairs({"A": "1", "B": "2"}), [["A", "1"], ["B", "2"]])

    def test_env_pairs_from_iterable(self):
        self.assertEqual(env_pairs([("A", "1")]), [["A", "1"]])

    def test_env_pairs_none(self):
        self.assertEqual(env_pairs(None), [])

    def test_unwrap_ok(self):
        self.assertEqual(unwrap({"ok": True, "data": {"x": 1}}), {"x": 1})

    def test_unwrap_maps_kinds(self):
        for kind, exc in [
            ("assertion", ExpectationError),
            ("usage", UsageError),
            ("no_session", NoSessionError),
            ("internal", InternalError),
        ]:
            with self.assertRaises(exc):
                unwrap({"ok": False, "kind": kind, "message": "boom"})

    def test_open_payload(self):
        c = CapturingClient("s")
        run(c.open(cols=120, rows=40, env={"K": "V"}))
        self.assertEqual(
            c.sent[0],
            {
                "kind": "open",
                "shell": None,
                "program": None,
                "cols": 120,
                "rows": 40,
                "cwd": None,
                "env": [["K", "V"]],
            },
        )

    def test_run_payload(self):
        c = CapturingClient("s")
        run(c.run("vim", "file.txt"))
        self.assertEqual(c.sent[0]["program"], ["vim", "file.txt"])
        self.assertIsNone(c.sent[0]["shell"])

    def test_submit_and_keys(self):
        c = CapturingClient("s")
        run(c.submit("echo hi"))
        run(c.keys("Ctrl+a"))
        run(c.press("Escape", "Enter"))
        self.assertEqual(c.sent[0], {"kind": "submit", "data": "echo hi"})
        self.assertEqual(c.sent[1], {"kind": "press", "keys": ["Ctrl+a"]})
        self.assertEqual(c.sent[2], {"kind": "press", "keys": ["Escape", "Enter"]})

    def test_kill_is_signal_kill(self):
        c = CapturingClient("s")
        run(c.kill())
        self.assertEqual(c.sent[0], {"kind": "signal", "name": "KILL"})

    def test_get_field_kebab(self):
        c = CapturingClient("s")
        c.reply = {"ok": True, "data": {"value": 0}}
        run(c.get_exit_code())
        self.assertEqual(c.sent[0], {"kind": "get", "field": "exit-code"})

    def test_mouse_click_payload(self):
        c = CapturingClient("s")
        run(c.mouse.click(on_text="OK", clicks=2))
        self.assertEqual(
            c.sent[0],
            {
                "kind": "mouse",
                "action": {
                    "op": "click",
                    "x": None,
                    "y": None,
                    "on_text": "OK",
                    "button": 0,
                    "clicks": 2,
                },
            },
        )

    def test_wait_text_defaults(self):
        c = CapturingClient("s")
        run(c.wait_text("done"))
        self.assertEqual(
            c.sent[0],
            {
                "kind": "wait_text",
                "text": "done",
                "regex": False,
                "full": False,
                "not": False,
            },
        )
        self.assertNotIn("timeout_ms", c.sent[0])

    def test_expect_text_strict_and_color(self):
        c = CapturingClient("s")
        run(c.expect_text("ERR", fg="#ff0000"))
        self.assertEqual(c.sent[0]["strict"], True)
        self.assertEqual(c.sent[0]["fg"], "#ff0000")
        self.assertNotIn("timeout_ms", c.sent[0])

    def test_wait_command_omits_timeout_when_unset(self):
        c = CapturingClient("s")
        run(c.wait_command())
        self.assertEqual(c.sent[0], {"kind": "wait_command"})


if __name__ == "__main__":
    unittest.main()
