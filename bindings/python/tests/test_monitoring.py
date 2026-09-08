import asyncio
import io
import json
import os
import unittest
from unittest import mock

from tui_test import MonitoringMetadata, MonitoringOptions, TuiTest, client, testing
from tui_test import _config as cfg


class MonitoringOptionsTests(unittest.TestCase):
    def setUp(self):
        self.env = mock.patch.dict(os.environ, {}, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_defaults_are_disabled_and_finite(self):
        expected = {
            "enabled": False,
            "wait_at_end": "never",
            "first_attach_timeout": 30_000,
            "hold_while_attached": True,
            "label": None,
            "metadata": {},
        }
        for value in (None, {}, MonitoringOptions()):
            self.assertEqual(cfg.resolve_monitoring(value), expected)

    def test_environment_and_explicit_precedence(self):
        with mock.patch.dict(os.environ, {
            "TUI_TEST_MONITORING": "true",
            "TUI_TEST_WAIT_AT_END": "failure",
            "TUI_TEST_FIRST_ATTACH_TIMEOUT": "45",
            "TUI_TEST_LABEL": "environment",
        }):
            env = cfg.resolve_monitoring()
            self.assertTrue(env["enabled"])
            self.assertEqual(env["wait_at_end"], "failure")
            self.assertEqual(env["first_attach_timeout"], 45)
            self.assertEqual(env["label"], "environment")
            explicit = cfg.resolve_monitoring(MonitoringOptions(
                enabled=False, wait_at_end="never", first_attach_timeout=None,
                hold_while_attached=False, label="explicit",
            ))
        self.assertFalse(explicit["enabled"])
        self.assertEqual(explicit["wait_at_end"], "never")
        self.assertIsNone(explicit["first_attach_timeout"])
        self.assertFalse(explicit["hold_while_attached"])
        self.assertEqual(explicit["label"], "explicit")

    def test_wait_policy_enables_unless_explicitly_disabled(self):
        for wait in ("failure", "always"):
            self.assertTrue(cfg.resolve_monitoring({"wait_at_end": wait})["enabled"])
            self.assertFalse(cfg.resolve_monitoring({
                "enabled": False, "wait_at_end": wait,
            })["enabled"])

    def test_explicit_values_override_invalid_environment(self):
        with mock.patch.dict(os.environ, {
            "TUI_TEST_WAIT_AT_END": "invalid",
            "TUI_TEST_FIRST_ATTACH_TIMEOUT": "invalid",
        }):
            options = cfg.resolve_monitoring({
                "wait_at_end": "never", "first_attach_timeout": 0,
            })
        self.assertEqual(options["first_attach_timeout"], 0)

    def test_invalid_option_types_and_fields(self):
        for value in (
            True, [], {"unknown": 1}, {"wait_at_end": "sometimes"},
            {"wait_at_end": None}, {"enabled": 1}, {"enabled": None},
            {"hold_while_attached": 0}, {"label": None}, {"label": 3},
            {"metadata": None}, {"metadata": {"testFile": "test.py"}},
            {"metadata": {"test_name": 7}}, {"metadata": {"worker": None}},
        ):
            with self.subTest(value=value), self.assertRaises((TypeError, ValueError)):
                cfg.resolve_monitoring(value)

    def test_timeout_is_strict_integer_not_bool(self):
        for value in (True, False, -1, 1.5, "42", 2**64):
            with self.subTest(value=value), self.assertRaises(TypeError):
                cfg.resolve_monitoring({"first_attach_timeout": value})
        for value in (0, 42, None):
            self.assertEqual(
                cfg.resolve_monitoring({"first_attach_timeout": value})["first_attach_timeout"],
                value,
            )

    def test_invalid_environment_timeout_is_rejected(self):
        for value in ("", "-1", "1.5", "true", "None"):
            with mock.patch.dict(os.environ, {"TUI_TEST_FIRST_ATTACH_TIMEOUT": value}):
                with self.subTest(value=value), self.assertRaises(ValueError):
                    cfg.resolve_monitoring()

    def test_environment_infinite_matches_shared_core_and_explicit_override(self):
        for value in ("infinite", " INFINITE "):
            with mock.patch.dict(os.environ, {"TUI_TEST_FIRST_ATTACH_TIMEOUT": value}):
                self.assertIsNone(cfg.resolve_monitoring()["first_attach_timeout"])
                self.assertEqual(
                    cfg.resolve_monitoring({"first_attach_timeout": 0})["first_attach_timeout"],
                    0,
                )
        with self.assertRaises(TypeError):
            cfg.resolve_monitoring({"first_attach_timeout": "infinite"})

    def test_metadata_dataclass_and_native_json(self):
        options = MonitoringOptions(
            enabled=True, label="login",
            metadata=MonitoringMetadata(
                test_file="test_login.py", test_name="test_password",
                framework="unittest", worker="2",
            ),
        )
        with mock.patch.object(client.native, "NativeSession") as constructor:
            TuiTest("test", monitoring=options)
        self.assertEqual(json.loads(constructor.call_args.args[3]), {
            "label": "login", "test_file": "test_login.py",
            "test_name": "test_password", "framework": "unittest", "worker": "2",
        })
        self.assertEqual(cfg.resolve_monitoring(MonitoringOptions(
            metadata=MonitoringMetadata(test_file="test.py")
        ))["metadata"], {"test_file": "test.py"})

    def test_disabled_does_not_pass_native_metadata(self):
        with mock.patch.object(client.native, "NativeSession") as constructor:
            TuiTest(monitoring={"enabled": False, "label": "ignored"})
        self.assertIsNone(constructor.call_args.args[3])

    def test_helper_passes_monitoring_through(self):
        options = MonitoringOptions(enabled=True)
        self.assertIs(
            testing._client_kwargs(testing.TerminalOptions(monitoring=options))["monitoring"],
            options,
        )


class FakeMonitoringSession:
    def __init__(self):
        self.calls = []
        self.waiting = asyncio.Event()
        self.release = asyncio.Event()
        self.monitor_error = None
        self.close_error = None
        self.open_error = None
        self.begin_options = None

    def begin_monitor_wait(self, outcome, timeout, hold):
        self.begin_options = (outcome, timeout, hold)
        self.calls.append(("begin", outcome))
        return (
            "process-id:exact-session-id", 7,
            "tui-test monitor --interactive --id process-id:exact-session-id",
        )

    async def wait_for_monitor(self, generation, timeout, hold):
        self.calls.append(("wait", generation, timeout, hold))
        self.waiting.set()
        if self.monitor_error is not None:
            raise self.monitor_error
        if timeout is None:
            await self.release.wait()
        else:
            try:
                await asyncio.wait_for(self.release.wait(), timeout / 1000)
            except asyncio.TimeoutError:
                return False
        return True

    def cancel_monitor_wait(self, generation):
        self.calls.append(("cancel", generation))
        self.release.set()

    async def close_monitor_target(self, generation):
        self.calls.append(("close_target", generation))
        if self.close_error is not None:
            raise self.close_error

    async def close(self):
        self.calls.append(("close",))
        if self.close_error is not None:
            raise self.close_error

    async def open(self, *args):
        if self.open_error is not None:
            raise self.open_error
        return {}


class MonitoringLifecycleTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        patcher = mock.patch.dict(os.environ, {}, clear=True)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.banner = io.StringIO()
        patcher = mock.patch.object(client.sys, "stderr", self.banner)
        patcher.start()
        self.addCleanup(patcher.stop)

    def terminal(self, **options):
        fake = FakeMonitoringSession()
        with mock.patch.object(client.native, "NativeSession", return_value=fake):
            term = TuiTest("test", monitoring={
                "wait_at_end": "failure",
                "first_attach_timeout": 0,
                **options,
            })
        return term, fake

    async def test_no_client_timeout_and_copyable_banner(self):
        term, fake = self.terminal(
            first_attach_timeout=5, label="login",
            metadata={"test_file": "test_login.py"},
        )
        await asyncio.wait_for(term.finish("failed"), 1)
        self.assertEqual(fake.calls[-1], ("close_target", 7))
        self.assertIn("waiting 5 ms", self.banner.getvalue())
        self.assertIn("login | test_login.py", self.banner.getvalue())
        self.assertIn(
            "[tui-test] tui-test monitor --interactive --id process-id:exact-session-id",
            self.banner.getvalue(),
        )

    async def test_original_exception_identity_and_traceback(self):
        term, fake = self.terminal()
        original = AssertionError("primary assertion")
        try:
            raise original
        except AssertionError:
            traceback = original.__traceback__
            try:
                await term.inspect_failure(original)
            except AssertionError as raised:
                self.assertIs(raised, original)
                frames = []
                current = raised.__traceback__
                while current is not None:
                    frames.append(current)
                    current = current.tb_next
                self.assertIn(traceback, frames)
            else:
                self.fail("original exception was not raised")
        self.assertEqual(fake.calls[-1], ("close_target", 7))

    async def test_banner_uses_native_safely_quoted_command_verbatim(self):
        term, fake = self.terminal()
        command = "tui-test monitor --interactive --id 'owner/session with spaces'"

        def begin(outcome, timeout, hold):
            return "owner/session with spaces", 7, command

        fake.begin_monitor_wait = begin
        await term.finish("failed")
        self.assertIn("[tui-test] " + command, self.banner.getvalue())

    async def test_context_manager_preserves_original_traceback(self):
        term, _ = self.terminal()
        original = AssertionError("context")
        tracebacks = []
        try:
            async with term:
                try:
                    raise original
                except AssertionError:
                    tracebacks.append(original.__traceback__)
                    raise
        except AssertionError as raised:
            self.assertIs(raised, original)
            self.assertIs(raised.__traceback__, tracebacks[0])
        else:
            self.fail("context swallowed the exception")

    async def test_concurrent_cleanup_cannot_bypass_active_inspection(self):
        term, fake = self.terminal(first_attach_timeout=None)
        original = AssertionError("hold")
        inspection = asyncio.create_task(term.inspect_failure(original))
        await fake.waiting.wait()
        testing.track_terminal(term)
        cleaners = [
            asyncio.create_task(term.close()),
            asyncio.create_task(term.close_quiet()),
            asyncio.create_task(term.__aexit__(None, None, None)),
            asyncio.create_task(testing.close_all_tracked()),
        ]
        await asyncio.sleep(0)
        self.assertTrue(all(not task.done() for task in cleaners))
        self.assertFalse(any(call[0].startswith("close") for call in fake.calls))
        fake.release.set()
        with self.assertRaises(AssertionError) as raised:
            await inspection
        self.assertIs(raised.exception, original)
        await asyncio.gather(*cleaners)
        self.assertEqual(fake.calls.count(("close_target", 7)), 1)
        self.assertEqual(fake.calls.count(("begin", "failed")), 1)

    async def test_cancelling_concurrent_close_does_not_cancel_inspection(self):
        term, fake = self.terminal(first_attach_timeout=None)
        inspection = asyncio.create_task(term.finish("failed"))
        await fake.waiting.wait()
        cleanup = asyncio.create_task(term.close())
        await asyncio.sleep(0)
        cleanup.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await cleanup
        self.assertNotIn(("cancel", 7), fake.calls)
        self.assertFalse(inspection.done())
        fake.release.set()
        await inspection

    async def test_global_cleanup_joins_existing_finish_task(self):
        term, fake = self.terminal(first_attach_timeout=None)
        inspection = asyncio.create_task(term.finish("failed"))
        await fake.waiting.wait()
        native_closed = []

        async def close_all():
            native_closed.append(True)

        with mock.patch.object(client.native, "close_all", create=True, new=close_all):
            cleanup = asyncio.create_task(client.close_all())
            await asyncio.sleep(0)
            self.assertFalse(cleanup.done())
            self.assertEqual(native_closed, [])
            fake.release.set()
            await asyncio.gather(inspection, cleanup)
        self.assertEqual(native_closed, [True])
        self.assertEqual(fake.calls.count(("close_target", 7)), 1)

    async def test_concurrent_finish_uses_one_authoritative_task(self):
        term, fake = self.terminal(first_attach_timeout=None)
        first = asyncio.create_task(term.finish("failed"))
        await fake.waiting.wait()
        second = asyncio.create_task(term.finish("failed"))
        await asyncio.sleep(0)
        fake.release.set()
        await asyncio.gather(first, second)
        self.assertEqual(fake.calls.count(("begin", "failed")), 1)
        self.assertEqual(fake.calls.count(("close_target", 7)), 1)

    async def test_cancelling_inspection_cancels_native_wait_and_cleans_up(self):
        term, fake = self.terminal(first_attach_timeout=None)
        inspection = asyncio.create_task(term.finish("failed"))
        await fake.waiting.wait()
        inspection.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await inspection
        await asyncio.wait_for(term.close(), 1)
        self.assertIn(("cancel", 7), fake.calls)
        self.assertEqual(fake.calls[-1], ("close_target", 7))

    async def test_interrupts_never_start_new_wait(self):
        for interrupted in (asyncio.CancelledError(), KeyboardInterrupt(), SystemExit(2)):
            term, fake = self.terminal(wait_at_end="always", first_attach_timeout=None)
            await term.__aexit__(type(interrupted), interrupted, None)
            await term.close()
            self.assertEqual(fake.calls, [("cancel", None), ("close",)])

    async def test_interrupt_does_not_wait_for_already_attached_cleanup(self):
        term, fake = self.terminal(wait_at_end="always", first_attach_timeout=None)
        async def close_attached():
            await fake.release.wait()

        fake.close = close_attached
        interrupted = KeyboardInterrupt()
        await asyncio.wait_for(term.__aexit__(KeyboardInterrupt, interrupted, None), 1)
        self.assertIn(("cancel", None), fake.calls)
        self.assertFalse(any(call[0] == "begin" for call in fake.calls))
        await asyncio.wait_for(term.close(), 1)

    async def test_primary_error_survives_secondary_errors(self):
        term, fake = self.terminal()
        fake.monitor_error = RuntimeError("monitor failed")
        fake.close_error = RuntimeError("cleanup failed")
        original = AssertionError("primary")
        with self.assertRaises(AssertionError) as raised:
            await term.finish("failed", error=original)
        self.assertIs(raised.exception, original)
        self.assertIn("monitor failed", self.banner.getvalue())
        self.assertIn("cleanup failed", self.banner.getvalue())

    async def test_secondary_errors_propagate_without_primary(self):
        for phase in ("monitor_error", "close_error"):
            term, fake = self.terminal()
            failure = RuntimeError(phase)
            setattr(fake, phase, failure)
            with self.subTest(phase=phase), self.assertRaises(RuntimeError) as raised:
                await term.finish("failed")
            self.assertIs(raised.exception, failure)

    async def test_synchronous_native_errors_are_converted(self):
        term, fake = self.terminal()

        def begin(outcome, timeout, hold):
            raise client.native.NativeNoSessionError("not registered")

        fake.begin_monitor_wait = begin
        with self.assertRaises(client.NoSessionError):
            await term.finish("failed")
        self.assertEqual(fake.calls, [("cancel", None), ("close",)])

    async def test_wait_policies_and_disabled_behavior(self):
        for options, outcome, wait in (
            ({"enabled": False}, "failed", False),
            ({"wait_at_end": "never", "enabled": True}, "failed", False),
            ({}, "passed", False),
            ({"wait_at_end": "always"}, "passed", True),
        ):
            term, fake = self.terminal(**options)
            await term.finish(outcome)
            self.assertEqual(any(call[0] == "begin" for call in fake.calls), wait)

    async def test_explicit_close_does_not_infer_failure_or_start_hold(self):
        term, fake = self.terminal(wait_at_end="always", first_attach_timeout=None)
        await term.close()
        self.assertEqual(fake.calls, [("close",)])

    async def test_finish_publishes_explicit_no_hold_without_first_attachment_wait(self):
        for policy, outcome in (("never", "failed"), ("failure", "passed")):
            term, fake = self.terminal(
                wait_at_end=policy, enabled=True,
                first_attach_timeout=None, hold_while_attached=False,
            )
            await asyncio.wait_for(term.finish(outcome), 1)
            self.assertEqual(fake.begin_options, (outcome, 0, False))
            self.assertIn(("wait", 7, 0, False), fake.calls)
            self.assertEqual(fake.calls[-1], ("close_target", 7))
        self.assertEqual(self.banner.getvalue(), "")

    async def test_terminal_helper_uses_failure_context(self):
        term, fake = self.terminal()
        with mock.patch.object(testing, "create_terminal", return_value=term):
            with self.assertRaises(AssertionError):
                async with testing.terminal():
                    raise AssertionError("helper")
        self.assertIn(("begin", "failed"), fake.calls)
        self.assertEqual(fake.calls[-1], ("close_target", 7))

    async def test_create_terminal_failure_uses_context_and_untracks(self):
        term, fake = self.terminal()
        original = RuntimeError("spawn failed")
        fake.open_error = original
        with mock.patch.object(testing, "TuiTest", return_value=term):
            with self.assertRaises(RuntimeError) as raised:
                await testing.create_terminal(retries=0)
        self.assertIs(raised.exception, original)
        self.assertIn(("begin", "failed"), fake.calls)
        self.assertEqual(testing.tracked_count(), 0)

    async def test_successful_helper_cleanup_errors_are_not_hidden(self):
        term, fake = self.terminal()
        fake.close_error = RuntimeError("cleanup")
        with mock.patch.object(testing, "create_terminal", return_value=term):
            with self.assertRaisesRegex(RuntimeError, "cleanup"):
                async with testing.terminal():
                    pass

    async def test_atexit_is_forceful_not_async_cleanup(self):
        term, fake = self.terminal(first_attach_timeout=None)
        testing.track_terminal(term)
        with mock.patch.object(testing, "_atexit_close_all") as close_all:
            testing._close_all_tracked_blocking()
        close_all.assert_called_once_with()
        self.assertEqual(fake.calls, [])
        self.assertEqual(testing.tracked_count(), 0)


if __name__ == "__main__":
    unittest.main()
