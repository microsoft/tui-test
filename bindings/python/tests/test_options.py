import asyncio
import os
import re
import unittest
from pathlib import Path
from unittest import mock

from shell_use import _config as cfg
from shell_use import _ephemeral as ephemeral
from shell_use import client
from shell_use.errors import ExpectationError, TerminalArtifact
from shell_use.types import Timeouts


def run(coro):
    return asyncio.run(coro)


class SocketPathTests(unittest.TestCase):
    def test_short_path_keeps_the_session_name(self):
        home = Path("/tmp/shell-use")
        self.assertEqual(
            cfg._socket_path_in(home, "work"),
            home / "work.sock",
        )

    def test_long_path_matches_the_rust_and_javascript_digest(self):
        home = Path(
            "/var/folders/9k/hd3xzq_s0mn1c7b2v8t4wxyz0000gn/T/"
            "shell-use-Ab12Cd34"
        )
        self.assertEqual(
            cfg._socket_path_in(home, "helpers-track-54321-9f8e7d6c-1"),
            home / "9ba800cbf25eaece.sock",
        )


class _CapturingClient(client.ShellUse):
    """Records payloads instead of touching the transport."""

    def __init__(self, *a, **k):
        super().__init__(*a, **k)
        self.sent = []
        self.reply = {}
        self.raise_kind = None

    async def send(self, payload):
        self.sent.append(payload)
        if self.raise_kind is not None:
            raise self.raise_kind
        return self.reply


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

    def test_timeouts_field_beats_omitted(self):
        both = {"command": 222}
        self.assertEqual(cfg.resolve_timeout("command", timeouts=both), 222)
        self.assertIsNone(cfg.resolve_timeout("idle", timeouts=both))

    def test_none_entry_in_timeouts_falls_through(self):
        self.assertIsNone(cfg.resolve_timeout("text", timeouts={"text": None}))

    def test_normalize_timeouts_accepts_dataclass_and_mapping(self):
        self.assertIsNone(cfg.normalize_timeouts(None))
        self.assertEqual(cfg.normalize_timeouts({"command": 42})["command"], 42)
        norm = cfg.normalize_timeouts(Timeouts(command=42))
        self.assertEqual(norm["command"], 42)
        self.assertIsNone(norm["idle"])

    def test_session_timeouts_payload_omits_when_empty(self):
        self.assertIsNone(cfg.session_timeouts_payload(None))
        self.assertIsNone(cfg.session_timeouts_payload({}))
        self.assertIsNone(cfg.session_timeouts_payload(Timeouts()))

    def test_session_timeouts_payload_keeps_only_set_classes(self):
        self.assertEqual(
            cfg.session_timeouts_payload(Timeouts(command=2000, ready=3000)),
            {"command": 2000, "ready": 3000},
        )
        self.assertEqual(cfg.session_timeouts_payload({"text": 100}), {"text": 100})


class ClientTimeoutPayloadTests(unittest.TestCase):
    def test_omits_timeout_ms_when_unconfigured(self):
        c = _CapturingClient("s")
        run(c.wait_idle())
        self.assertNotIn("timeout_ms", c.sent[0])
        run(c.wait_command())
        self.assertNotIn("timeout_ms", c.sent[1])
        run(c.wait_exit())
        self.assertNotIn("timeout_ms", c.sent[2])
        run(c.wait_ready())
        self.assertNotIn("timeout_ms", c.sent[3])

    def test_client_timeouts_field_threads_into_payload(self):
        c = _CapturingClient("s", timeouts=Timeouts(command=2000, idle=1500))
        run(c.wait_command())
        self.assertEqual(c.sent[0]["timeout_ms"], 2000)
        run(c.wait_idle())
        self.assertEqual(c.sent[1]["timeout_ms"], 1500)
        run(c.wait_exit())
        self.assertNotIn("timeout_ms", c.sent[2])

    def test_per_call_beats_client_timeouts_which_beats_omitted(self):
        c = _CapturingClient("s", timeouts=Timeouts(idle=1000))
        run(c.wait_idle(timeout=50))  # per-call wins
        self.assertEqual(c.sent[0]["timeout_ms"], 50)
        run(c.wait_idle())  # client-level default applies
        self.assertEqual(c.sent[1]["timeout_ms"], 1000)
        run(c.wait_command())  # nothing configured -> omitted
        self.assertNotIn("timeout_ms", c.sent[2])

    def test_text_class_covers_wait_text_and_expect_text(self):
        c = _CapturingClient("s", timeouts=Timeouts(text=1234))
        run(c.wait_text("x"))
        self.assertEqual(c.sent[0]["timeout_ms"], 1234)
        run(c.expect_text("x"))
        self.assertEqual(c.sent[1]["timeout_ms"], 1234)

    def test_command_class_covers_wait_command_and_expect_exit_code(self):
        c = _CapturingClient("s", timeouts=Timeouts(command=2222))
        run(c.wait_command())
        self.assertEqual(c.sent[0]["timeout_ms"], 2222)
        run(c.expect_exit_code(0))
        self.assertEqual(c.sent[1]["timeout_ms"], 2222)

    def test_expect_exit_code_sends_a_per_call_timeout(self):
        c = _CapturingClient("s")
        run(c.expect_exit_code(0, timeout=777))
        self.assertEqual(c.sent[0]["timeout_ms"], 777)
        run(c.expect_exit_code(0))
        self.assertNotIn("timeout_ms", c.sent[1])

    def test_plain_dict_timeouts_supported(self):
        c = _CapturingClient("s", timeouts={"command": 4321})
        run(c.wait_command())
        self.assertEqual(c.sent[0]["timeout_ms"], 4321)


class OpenSessionTimeoutTests(unittest.TestCase):
    def test_open_omits_timeouts_when_unset(self):
        c = _CapturingClient("s")
        run(c.open())
        self.assertNotIn("timeouts", c.sent[0])

    def test_open_omits_timeouts_when_all_classes_none(self):
        c = _CapturingClient("s")
        run(c.open(timeouts=Timeouts()))
        self.assertNotIn("timeouts", c.sent[0])

    def test_open_forwards_only_set_classes(self):
        c = _CapturingClient("s")
        run(c.open(timeouts=Timeouts(text=1000, ready=2000)))
        self.assertEqual(c.sent[0]["timeouts"], {"text": 1000, "ready": 2000})

    def test_open_timeouts_accepts_plain_dict(self):
        c = _CapturingClient("s")
        run(c.open(timeouts={"command": 5000}))
        self.assertEqual(c.sent[0]["timeouts"], {"command": 5000})

    def test_run_forwards_session_timeouts(self):
        c = _CapturingClient("s")
        run(c.run("vim", timeouts=Timeouts(idle=1500)))
        self.assertEqual(c.sent[0]["timeouts"], {"idle": 1500})


class UniqueSessionTests(unittest.TestCase):
    def test_format_and_uniqueness(self):
        a = ephemeral.unique_session()
        b = ephemeral.unique_session()
        self.assertTrue(a.startswith("shell-use-"))
        self.assertNotEqual(a, b)

    def test_sanitizes_unsafe_characters(self):
        name = ephemeral.unique_session("a b/c\\d:e.f")
        self.assertIsNotNone(re.fullmatch(r"[A-Za-z0-9_-]+", name))

    def test_capped_at_64(self):
        name = ephemeral.unique_session("x" * 500)
        self.assertLessEqual(len(name), 64)
        self.assertRegex(name, r"-\d+-[0-9a-f]+-\d+$")

    def test_default_prefix(self):
        self.assertTrue(ephemeral.unique_session("").startswith("shell-use-"))


class WaitReadyPayloadTests(unittest.TestCase):
    def test_wait_ready_omits_timeout_when_unset(self):
        c = _CapturingClient("s")
        run(c.wait_ready())
        self.assertEqual(c.sent[0], {"kind": "wait_ready"})

    def test_open_omits_wait_ready_when_none(self):
        c = _CapturingClient("s")
        run(c.open())
        self.assertNotIn("wait_ready", c.sent[0])

    def test_open_forwards_wait_ready(self):
        c = _CapturingClient("s")
        run(c.open(wait_ready=True))
        self.assertEqual(c.sent[0]["wait_ready"], True)
        run(c.run("vim", wait_ready=False))
        self.assertEqual(c.sent[1]["wait_ready"], False)


class RetryTests(unittest.TestCase):
    def test_retries_reattempt_and_reraise_last(self):
        calls = {"n": 0}

        class Flaky(client.ShellUse):
            async def send(self, payload):
                calls["n"] += 1
                raise RuntimeError("attempt %d" % calls["n"])

            async def close_quiet(self):
                pass

        c = Flaky("s")
        with self.assertRaises(RuntimeError) as raised:
            run(c.open(retries=2))
        self.assertEqual(calls["n"], 3)
        self.assertEqual(str(raised.exception), "attempt 3")

    def test_no_retries_single_attempt(self):
        calls = {"n": 0}

        class Flaky(client.ShellUse):
            async def send(self, payload):
                calls["n"] += 1
                raise RuntimeError("boom")

            async def close_quiet(self):
                pass

        c = Flaky("s")
        with self.assertRaises(RuntimeError):
            run(c.open())
        self.assertEqual(calls["n"], 1)


class MessagePrefixTests(unittest.TestCase):
    def _prefix_for(self, method_name, *args, **kwargs):
        c = _CapturingClient("s")
        c.raise_kind = ExpectationError("boom")
        method = getattr(c, method_name)
        with self.assertRaises(ExpectationError) as raised:
            run(method(*args, **kwargs))
        return str(raised.exception)

    def test_all_wait_and_expect_methods_prefix(self):
        cases = {
            "wait_text": ("x",),
            "wait_idle": (),
            "wait_command": (),
            "wait_exit": (),
            "wait_ready": (),
            "expect_text": ("x",),
            "expect_output": ("x",),
            "expect_snapshot": ("x",),
        }
        for name, args in cases.items():
            message = self._prefix_for(name, *args)
            self.assertTrue(
                message.startswith(name + ": "),
                "%s did not prefix: %r" % (name, message),
            )

    def test_expect_exit_code_prefixes(self):
        c = _CapturingClient("s")
        c.raise_kind = ExpectationError("boom")
        with self.assertRaises(ExpectationError) as raised:
            run(c.expect_exit_code(0))
        self.assertTrue(str(raised.exception).startswith("expect_exit_code: "))


class ArtifactCaptureTests(unittest.TestCase):
    def test_no_artifacts_leaves_terminal_none(self):
        c = _CapturingClient("s")
        c.raise_kind = ExpectationError("nope\n\nTerminal content:\n╭──╮\n╰──╯")
        with self.assertRaises(ExpectationError) as raised:
            run(c.wait_text("x"))
        self.assertIsNone(raised.exception.terminal)

    def test_text_mode_captures_terminal_text_only(self):
        c = _CapturingClient("s", artifacts={"dir": "unused", "on_failure": "text"})
        c.raise_kind = ExpectationError("nope\n\nTerminal content:\n╭──╮\n╰──╯")
        with self.assertRaises(ExpectationError) as raised:
            run(c.wait_text("x"))
        terminal = raised.exception.terminal
        self.assertIsInstance(terminal, TerminalArtifact)
        self.assertIn("╭──╮", terminal.text)
        self.assertIsNone(terminal.screenshot)

    def test_capture_never_masks_original_error(self):
        c = _CapturingClient("s", artifacts={"dir": "unused", "on_failure": "svg"})
        c.raise_kind = ExpectationError("nope\n\nTerminal content:\n╭──╮\n╰──╯")

        async def boom(*a, **k):
            raise RuntimeError("screenshot exploded")

        c.screenshot = boom
        with self.assertRaises(ExpectationError):
            run(c.wait_text("x"))


class CloseIdempotencyTests(unittest.TestCase):
    def test_close_is_idempotent_without_daemon(self):
        async def scenario():
            with mock.patch.object(client.transport, "can_connect") as cc:
                async def _false(session, home):
                    return False

                cc.side_effect = _false
                c = client.ShellUse("idem", home="ignored-dir")
                await c.close()
                await c.close()
                await c.close_quiet()

        run(scenario())

    def test_unused_temp_home_close_is_noop(self):
        c = client.ShellUse.ephemeral("worker")
        run(c.close())
        run(c.close())
        self.assertIsNone(c._temp_home)

    def test_close_only_talks_to_the_daemon_once(self):
        calls = []

        async def _connect(session, home):
            calls.append(session)
            return False

        async def scenario():
            with mock.patch.object(client.transport, "can_connect", _connect):
                c = client.ShellUse("idem-once", home="ignored-dir")
                await c.close()
                await c.close()

        run(scenario())
        self.assertEqual(len(calls), 1)


class IsolatedHomeTests(unittest.TestCase):
    def test_isolated_provisions_a_private_directory(self):
        c = client.ShellUse("s", isolated=True)
        home = c._ensure_home()
        try:
            self.assertIsNotNone(home)
            self.assertEqual(home, c._temp_home)
            self.assertTrue(os.path.isdir(home))
        finally:
            c._cleanup_temp_home()
        self.assertFalse(os.path.exists(home))

    def test_a_directory_named_temp_is_just_a_path(self):
        c = client.ShellUse("s", home="temp")
        self.assertEqual(c._ensure_home(), "temp")
        self.assertIsNone(c._temp_home)

    def test_shell_use_home_env_is_honoured_verbatim(self):
        with mock.patch.dict("os.environ", {"SHELL_USE_HOME": "temp"}):
            c = client.ShellUse("s")
            self.assertEqual(c._ensure_home(), "temp")
            self.assertIsNone(c._temp_home)

    def test_isolated_ignores_home(self):
        c = client.ShellUse("s", home="ignored-dir", isolated=True)
        home = c._ensure_home()
        try:
            self.assertNotEqual(home, "ignored-dir")
        finally:
            c._cleanup_temp_home()

    def test_ephemeral_is_isolated(self):
        c = client.ShellUse.ephemeral("worker")
        self.assertTrue(c._isolated)
        self.assertNotEqual(c.session, "default")


class UnknownTimeoutClassTests(unittest.TestCase):
    def test_normalize_rejects_unknown_keys(self):
        with self.assertRaises(ValueError) as raised:
            cfg.normalize_timeouts({"comand": 100})
        self.assertIn("comand", str(raised.exception))

    def test_open_rejects_unknown_keys(self):
        c = _CapturingClient("s")
        with self.assertRaises(ValueError):
            run(c.open(timeouts={"txt": 100}))

    def test_known_keys_still_pass(self):
        self.assertEqual(
            cfg.session_timeouts_payload({"text": 1, "ready": 2}),
            {"text": 1, "ready": 2},
        )


if __name__ == "__main__":
    unittest.main()
