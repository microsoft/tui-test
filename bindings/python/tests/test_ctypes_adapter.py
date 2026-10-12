import asyncio
import ctypes as C
import json
import threading
import unittest
from unittest.mock import Mock, patch

from tui_test import _abi as abi
from tui_test import _native as native


class OwnershipTests(unittest.TestCase):
    def test_null_and_empty_strings_remain_distinct(self):
        missing, empty = native._string(None), native._string("")
        self.assertFalse(missing.data)
        self.assertTrue(empty.data)
        self.assertEqual(empty.len, 0)
        self.assertIsNone(native._text(missing))
        self.assertEqual(native._text(empty), "")

    def test_result_is_freed_after_success_error_and_conversion_failure(self):
        for code, text, exception in (
            (0, "copied", None),
            (2, "bad argument", native.NativeUsageError),
            (0, "copied", IndexError),
        ):
            with self.subTest(code=code, exception=exception):
                result = abi.TuiResult()
                result.error_kind = code
                result.error_message = native._string(text)
                result.text = native._string(text)
                result.snapshot = 99
                pointer = C.pointer(result)
                library = Mock()
                library.tui_text.return_value = pointer
                with patch.object(native, "_load", return_value=library):
                    kind = "snapshot" if exception is IndexError else "text"
                    if exception:
                        with self.assertRaises(exception):
                            native._invoke("text", (), kind)
                    else:
                        self.assertEqual(native._invoke("text", (), kind), "copied")
                library.tui_result_free.assert_called_once_with(pointer)

    def test_packed_screen_owns_immutable_copied_bytes(self):
        result = abi.TuiResult()
        result.text = native._string("UTF-8: café\n")
        result.size = abi.TuiSize(80, 24)
        library = Mock()
        library.tui_packed_screen.return_value = C.pointer(result)

        def overwrite(pointer):
            C.memset(pointer.contents.text.data, ord("x"), pointer.contents.text.len)

        library.tui_result_free.side_effect = overwrite
        with patch.object(native, "_load", return_value=library):
            screen, cols, rows = native._invoke("packed_screen", (), "packed")
        self.assertEqual((cols, rows), (80, 24))
        self.assertTrue(screen.readonly)
        self.assertEqual(screen.tobytes(), "UTF-8: café\n".encode())

    def test_error_details_and_artifact_survive_result_release(self):
        result = abi.TuiResult()
        result.error_kind = 1
        result.error_message = native._string("not found")
        result.error_details = native._string('{"reason":"timeout"}')
        result.error_artifact = native._string('{"directory":"artifacts"}')
        library = Mock()
        library.tui_expect_locator.return_value = C.pointer(result)
        with patch.object(native, "_load", return_value=library):
            with self.assertRaises(native.NativeAssertionError) as caught:
                native._invoke("expect_locator", ())
        envelope = json.loads(caught.exception._tui_test_error_json)
        self.assertEqual(envelope["details"], {"reason": "timeout"})
        self.assertEqual(envelope["artifact"], {"directory": "artifacts"})
        library.tui_result_free.assert_called_once()

    def test_abi_version_mismatch_is_structured(self):
        library = Mock()
        library.tui_abi_version.return_value = abi.ABI_VERSION + 1
        with patch.object(native, "_library", None), patch.object(C, "CDLL", return_value=library):
            with self.assertRaisesRegex(native.NativeInternalError, "ABI version mismatch"):
                native._load()

    def test_locator_translation_does_not_mutate_input(self):
        expression = {
            "nodes": [{"kind": "text", "text": "x", "occurrence": "nth", "nth": 2}],
            "root": 0,
        }
        query = native._query(expression)
        self.assertEqual(expression["nodes"][0]["nth"], 2)
        self.assertEqual(expression["nodes"][0]["occurrence"], "nth")
        self.assertEqual(json.loads(native._text(query.json))["nodes"][0]["occurrence"], {"nth": 2})

    def test_integer_validation_precedes_narrowing(self):
        class BooleanIndex:
            def __index__(self):
                return True

        for value in (-1, 65536, True, 1.5, BooleanIndex()):
            with self.subTest(value=value):
                with self.assertRaises(native.NativeUsageError):
                    native._integer(value, "cols", 16)


class WorkerTests(unittest.TestCase):
    def test_waits_do_not_starve_close(self):
        async def scenario():
            release = threading.Event()
            started = 0
            lock = threading.Lock()
            all_started = asyncio.get_running_loop().create_future()
            loop = asyncio.get_running_loop()

            def wait():
                nonlocal started
                with lock:
                    started += 1
                    if started == 40:
                        loop.call_soon_threadsafe(all_started.set_result, None)
                if not release.wait(10):
                    raise AssertionError("close was starved")

            waits = [asyncio.create_task(native._background(wait)) for _ in range(40)]
            try:
                await asyncio.wait_for(all_started, 5)
                await asyncio.wait_for(native._background(release.set), 2)
                await asyncio.gather(*waits)
            finally:
                release.set()
                await asyncio.gather(*waits, return_exceptions=True)

        asyncio.run(scenario())

    def test_cancelled_spawn_does_not_close_replacement_session(self):
        async def scenario(method):
            loop = asyncio.get_running_loop()
            started = loop.create_future()
            release = threading.Event()
            workers = []
            thread_class = threading.Thread
            generation = 0
            current = None

            def thread(*args, **kwargs):
                worker = thread_class(*args, **kwargs)
                workers.append(worker)
                return worker

            def invoke(name, args, kind=None):
                nonlocal generation, current
                if name == "close":
                    current = None
                elif name in ("open_ptr", "run_ptr", "restart"):
                    generation += 1
                    current = generation
                    result = {"generation": generation}
                    if generation == 1:
                        loop.call_soon_threadsafe(started.set_result, None)
                        if not release.wait(10):
                            raise AssertionError("spawn did not finish")
                    return result

            session = native.NativeSession("same-name")
            options = (None, 80, 24, None, [], None, False, None, [],
                       None, None, None, None, None, None)
            calls = {
                "open": lambda: session.open("bash", *options),
                "run": lambda: session.run("program", [], *options),
                "restart": lambda: session.restart(0),
            }
            with patch.object(native, "_invoke", side_effect=invoke), \
                    patch.object(native.threading, "Thread", side_effect=thread):
                task = asyncio.create_task(calls[method]())
                try:
                    await asyncio.wait_for(started, 5)
                    task.cancel()
                    with self.assertRaises(asyncio.CancelledError):
                        await task
                    await session.close()
                    await calls[method]()
                    self.assertEqual(current, 2)
                    release.set()
                    for worker in workers:
                        worker.join(5)
                        self.assertFalse(worker.is_alive())
                    self.assertEqual(current, 2)
                finally:
                    release.set()
                    for worker in workers:
                        worker.join(5)

        for method in ("open", "run", "restart"):
            with self.subTest(method=method):
                asyncio.run(scenario(method))

    def test_cancelled_call_still_frees_its_result_once(self):
        async def scenario():
            loop = asyncio.get_running_loop()
            started = loop.create_future()
            release = threading.Event()
            workers = []
            result = abi.TuiResult()
            result.text = native._string("copied")
            pointer = C.pointer(result)
            library = Mock()

            def text(*args):
                workers.append(threading.current_thread())
                loop.call_soon_threadsafe(started.set_result, None)
                if not release.wait(10):
                    raise AssertionError("native call did not finish")
                return pointer

            library.tui_text.side_effect = text
            try:
                with patch.object(native, "_load", return_value=library):
                    task = asyncio.create_task(native.NativeSession("cancelled").text(False))
                    await asyncio.wait_for(started, 5)
                    task.cancel()
                    with self.assertRaises(asyncio.CancelledError):
                        await task
                    release.set()
                    workers[0].join(5)
                    self.assertFalse(workers[0].is_alive())
                    library.tui_result_free.assert_called_once_with(pointer)
            finally:
                release.set()
                for worker in workers:
                    worker.join(5)

        asyncio.run(scenario())


if __name__ == "__main__":
    unittest.main()
