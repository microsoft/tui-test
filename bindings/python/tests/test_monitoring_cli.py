import asyncio
import functools
import json
import os
import subprocess
import sys
import unittest

from tui_test import TuiTest


CLI = os.environ.get("TUI_TEST_BIN")
READY = "PYTHON_MONITOR_READY"
RECEIVED = "PYTHON_MONITOR_RECEIVED:"
ECHO_CHILD = (
    "import sys\n"
    "print({!r}, flush=True)\n"
    "for line in sys.stdin:\n"
    "    print({!r} + line.strip(), flush=True)\n"
).format(READY, RECEIVED)


@unittest.skipUnless(CLI, "TUI_TEST_BIN is required for native-to-CLI monitoring")
class MonitoringCliTests(unittest.IsolatedAsyncioTestCase):
    async def test_no_hold_close_finishes_while_read_only_viewer_is_connected(self):
        for method in ("close", "close_quiet"):
            target = TuiTest.ephemeral(
                "python-no-hold", recording={"mode": "disabled"},
                monitoring={"enabled": True, "wait_at_end": "never", "hold_while_attached": False},
            )
            viewer = TuiTest.ephemeral("python-no-hold-viewer", monitoring={"enabled": False})
            try:
                await target.run(sys.executable, "-u", "-c", ECHO_CHILD, wait_ready=False)
                result = await asyncio.get_running_loop().run_in_executor(
                    None, functools.partial(
                        subprocess.run, [CLI, "sessions", "--json"],
                        capture_output=True, encoding="utf-8", timeout=5, check=True,
                    ),
                )
                entry = next(item for item in json.loads(result.stdout)["details"]
                             if item["session"] == target.session and item.get("pid") == os.getpid())
                await viewer.run(CLI, "monitor", "--id", entry["id"], wait_ready=False)
                await viewer.get_by_text("q quit").first().expect(timeout=10000)
                await asyncio.wait_for(getattr(target, method)(), 5)
                await viewer.wait_exit(timeout=10000)
            finally:
                await asyncio.wait_for(viewer.close_quiet(), 5)
                await asyncio.wait_for(target.close_quiet(), 5)

    async def test_same_instance_restart_leaves_an_infinite_inspection(self):
        target = TuiTest.ephemeral(
            "python-same-restart", recording={"mode": "disabled"},
            monitoring={"enabled": True, "wait_at_end": "always", "first_attach_timeout": None},
        )
        finishing = None
        try:
            await target.run(sys.executable, "-u", "-c", ECHO_CHILD, wait_ready=False)
            finishing = asyncio.create_task(target.finish("passed"))
            await asyncio.sleep(0)
            await asyncio.wait_for(
                target.run(sys.executable, "-u", "-c", ECHO_CHILD, wait_ready=False, restart=True), 10,
            )
            await asyncio.wait_for(finishing, 5)
            await target.get_by_text(READY).first().expect(timeout=5000)
        finally:
            if finishing is not None and not finishing.done():
                finishing.cancel()
                try:
                    await finishing
                except asyncio.CancelledError:
                    pass
            await asyncio.wait_for(target.close_quiet(), 5)

    async def _waiting_session(self, target, inspection):
        loop = asyncio.get_running_loop()
        deadline = loop.time() + 15
        while loop.time() < deadline:
            result = await loop.run_in_executor(
                None,
                functools.partial(
                    subprocess.run,
                    [CLI, "sessions", "--json", "--waiting"],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    encoding="utf-8",
                    errors="replace",
                    timeout=5,
                    check=False,
                ),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            details = json.loads(result.stdout)["details"]
            for entry in details:
                if entry["session"] == target.session and entry["pid"] == os.getpid():
                    self.assertEqual(entry["ownerType"], "process")
                    self.assertEqual(entry["status"], "waiting-for-attach")
                    self.assertEqual(entry["outcome"], "failed")
                    self.assertEqual(entry["label"], "python-cli-interop")
                    self.assertEqual(entry["framework"], "unittest")
                    self.assertFalse(inspection.done(), "inspection ended before attachment")
                    return entry["id"]
            self.assertFalse(inspection.done(), "inspection ended before discovery")
            await asyncio.sleep(0.05)
        self.fail("CLI did not discover the waiting Python-owned target")

    async def test_interactive_cli_holds_original_python_failure_until_detach(self):
        target = TuiTest.ephemeral(
            "python-monitor-target",
            recording={"mode": "disabled"},
            monitoring={
                "enabled": True,
                "wait_at_end": "failure",
                "first_attach_timeout": 60_000,
                "hold_while_attached": True,
                "label": "python-cli-interop",
                "metadata": {
                    "test_file": __file__,
                    "test_name": self._testMethodName,
                    "framework": "unittest",
                },
            },
        )
        viewer = TuiTest.ephemeral(
            "python-monitor-viewer",
            recording={"mode": "disabled"},
            monitoring={
                "enabled": False, "wait_at_end": "never", "first_attach_timeout": 0,
            },
        )
        inspection = None
        cleanup = None
        original = AssertionError("original Python failure held for CLI inspection")
        try:
            raise original
        except AssertionError:
            original_traceback = original.__traceback__

        async def inspect():
            # Observe failure immediately rather than leaving a rejected task
            # unhandled while the test drives a separate native PTY.
            try:
                await target.inspect_failure(original)
            except BaseException as error:
                return error, error.__traceback__
            self.fail("inspection swallowed the original failure")

        async def close():
            try:
                await target.close()
            except BaseException as error:
                return error
            return None

        try:
            await asyncio.wait_for(
                target.run(
                    sys.executable, "-u", "-c", ECHO_CHILD,
                    wait_ready=False, cols=80, rows=24,
                ),
                15,
            )
            await target.get_by_text(READY).first().expect(timeout=10_000)
            inspection = asyncio.create_task(inspect())
            attach_id = await self._waiting_session(target, inspection)
            cleanup = asyncio.create_task(close())
            await asyncio.sleep(0)
            self.assertFalse(cleanup.done(), "concurrent cleanup bypassed the hold")

            await asyncio.wait_for(
                viewer.run(
                    CLI, "monitor", "--interactive", "--id", attach_id,
                    wait_ready=False, cols=100, rows=35,
                ),
                15,
            )
            await viewer.get_by_text("Ctrl+] detach").first().expect(timeout=10_000)
            await viewer.get_by_text(READY).first().expect(timeout=10_000)
            await viewer.write("before-resize")
            await viewer.press("Enter")
            await target.get_by_text(RECEIVED + "before-resize").first().expect(
                timeout=10_000
            )
            self.assertFalse(inspection.done(), "forwarding input released inspection")
            self.assertFalse(cleanup.done(), "forwarding input bypassed cleanup gating")

            await viewer.resize(92, 30)
            await viewer.get_by_text("Ctrl+] detach").first().expect(timeout=10_000)
            await viewer.write("after-resize")
            await viewer.press("Enter")
            await target.get_by_text(RECEIVED + "after-resize").first().expect(
                timeout=10_000
            )
            self.assertFalse(inspection.done(), "viewer resize released inspection")
            self.assertFalse(cleanup.done(), "viewer resize bypassed cleanup gating")

            await viewer.press("Ctrl+]")
            await viewer.wait_exit(timeout=10_000)
            self.assertEqual((await viewer.state()).exited, 0)
            raised, traceback = await asyncio.wait_for(inspection, 10)
            self.assertIs(raised, original)
            while traceback is not None and traceback is not original_traceback:
                traceback = traceback.tb_next
            self.assertIs(traceback, original_traceback)
            self.assertIsNone(await asyncio.wait_for(cleanup, 10))
        finally:
            # Stop the viewer before target cleanup so a failed assertion cannot
            # leave the native hold waiting on this test's own CLI attachment.
            try:
                await asyncio.wait_for(viewer.close_quiet(), 5)
            finally:
                for task in (inspection, cleanup):
                    if task is not None:
                        if not task.done():
                            task.cancel()
                        try:
                            await asyncio.wait_for(task, 5)
                        except (Exception, asyncio.CancelledError):
                            pass
                await asyncio.wait_for(target.close_quiet(), 5)


if __name__ == "__main__":
    unittest.main()
