import asyncio
import sys
import unittest

from tui_test import NoSessionError, TuiTest, UsageError, Viewport, _native, unique_session


class ScrollbackTests(unittest.TestCase):
    def test_signed_integer_validation(self):
        async def scenario():
            terminal = TuiTest.ephemeral()
            native = _native.NativeSession(unique_session("scroll-validation"))
            for amount in (1.5, float("nan"), float("inf"), 2147483648, -2147483649, "1", True):
                with self.subTest(amount=amount):
                    with self.assertRaises(UsageError):
                        await terminal.scroll(amount)
                    with self.assertRaises(_native.NativeUsageError):
                        await native.scroll(amount)
            for amount in (0, 1, -1, -2147483648, 2147483647):
                with self.assertRaises(NoSessionError):
                    await terminal.scroll(amount)
        asyncio.run(scenario())

    def test_history_view_preserves_full_text(self):
        async def scenario():
            async with TuiTest.ephemeral(backend="alacritty") as terminal:
                await terminal.run(sys.executable, "-u", "-c",
                    "import sys; [print('history-'+str(i)) for i in range(40)]; sys.stdin.read()",
                    rows=5, cols=40)
                await terminal.get_by_text("history-39").expect()
                full = await terminal.text(full=True)
                live = await terminal.text()
                bottom: Viewport = await terminal.get_viewport()
                self.assertEqual(bottom["offset"], 0)
                self.assertGreater(bottom["history"], 0)
                await terminal.scroll(2)
                self.assertEqual((await terminal.get_viewport())["offset"], 2)
                await terminal.scroll(-1)
                self.assertEqual((await terminal.get_viewport())["offset"], 1)
                await terminal.scroll_to_top()
                top = await terminal.get_viewport()
                self.assertEqual(top["offset"], top["history"])
                self.assertIn("history-0", await terminal.text())
                self.assertEqual(await terminal.text(full=True), full)
                await terminal.scroll_to_bottom()
                self.assertEqual(await terminal.text(), live)
                await terminal.scroll_to_top()
                await terminal.keyboard.press("a")
                self.assertEqual((await terminal.get_viewport())["offset"], 0)
        asyncio.run(scenario())
