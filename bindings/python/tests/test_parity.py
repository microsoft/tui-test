import asyncio
import json
import unittest
from unittest.mock import AsyncMock, patch

from tui_test import (
    AutomaticRecording, ExecutionContext, FontFamilies, PaddingSides, RenderStyle,
    TextAnchor, TextScope, TextStyle, TraceOptions, TuiTest, WindowStyle,
)
from tui_test import _native


class ParityTests(unittest.TestCase):
    def test_constructor_rendering_defaults_are_owned_across_spawns(self):
        for method in ("open", "run"):
            with self.subTest(method=method):
                style = RenderStyle(
                    font_size=24,
                    font=FontFamilies(family="monospace", files=["original.ttf"]),
                    content_padding=PaddingSides(left=0),
                    window=WindowStyle(title_bar=False),
                )
                terminal = TuiTest("parity", style=style)
                style.font_size = 48
                style.font.family = "serif"
                style.font.files[0] = "changed.ttf"
                style.content_padding.left = 10
                style.window.title_bar = True
                captured = []

                def capture(name, arguments, kind):
                    native_style = arguments[1]._obj.style
                    captured.append((
                        native_style.font_size.value,
                        _native._text(native_style.font.family),
                        _native._text(native_style.font.files[0]),
                        native_style.content_padding.sides.left.value,
                        native_style.window.title_bar.value,
                    ))
                    return {}

                async def scenario():
                    args = ("example",) if method == "run" else ()
                    await getattr(terminal, method)(*args)
                    await getattr(terminal, method)(*args, style=style)
                    await getattr(terminal, method)(*args)

                with patch.object(_native, "_invoke", side_effect=capture):
                    asyncio.run(scenario())
                original = (24, "monospace", "original.ttf", 0, False)
                self.assertEqual(captured, [
                    original, (48, "serif", "changed.ttf", 10, True), original,
                ])
                self.assertEqual(style.font.files, ["changed.ttf"])

    def test_scoped_text_and_whole_match_style_are_immutable(self):
        terminal = TuiTest("parity")
        scope = TextScope(after=TextAnchor("Start", regex=True, occurrence=1),
                          before=TextAnchor("End"))
        style = TextStyle(bold=True)
        text = terminal.get_by_text("Save", scope=scope).with_style(style)
        scope.after.text = "changed"
        style.bold = False
        node = text._query.current()
        self.assertEqual(node["scope"]["after"], {
            "text": "Start", "regex": True, "occurrence": {"nth": 1},
        })
        self.assertEqual(node["scope"]["before"]["occurrence"], "unique")
        self.assertTrue(node["match_style"]["bold"])
        self.assertNotIn("style", node)

    def test_composite_relative_options_preserve_owners_and_original(self):
        terminal = TuiTest("parity")
        parent = terminal.get_by_text("Header")
        composite = terminal.get_by_text("Save").or_(terminal.get_by_link("test:save"))
        relative = composite.relative_to(parent, "after").with_style(TextStyle(italic=False))
        node = relative._query.current()
        self.assertEqual(node["kind"], "or")
        self.assertEqual(node["within"], 0)
        self.assertEqual(node["direction"], "after")
        self.assertFalse(node["match_style"]["italic"])
        self.assertNotIn("within", composite._query.current())
        self.assertLess(node["left"], relative._query.root)
        with self.assertRaises(ValueError):
            composite.relative_to(TuiTest("parity").get_by_text("Header"))
        filtered = composite.filter(has=terminal.get_by_text("Save"))
        self.assertEqual(filtered.relative_to(parent)._query.current()["kind"], "filter")

    def test_context_and_recording_overrides_do_not_change_defaults(self):
        context = ExecutionContext(operation_name="custom",
                                   diagnostic_context={"case": "save"},
                                   trace=TraceOptions(mode="off"),
                                   screen_history_limit=5)
        terminal = TuiTest("parity", context=context)
        context.diagnostic_context["case"] = "changed"
        self.assertEqual(json.loads(_native._text(terminal._native._session()))[
            "diagnostic_context"], {"case": "save"})
        with patch.object(terminal._native, "run", new_callable=AsyncMock) as run:
            asyncio.run(terminal.run(
                "example", recording=AutomaticRecording(directory="captures"),
                context=ExecutionContext(diagnostic_context={"phase": "run"}),
            ))
        self.assertEqual(run.call_args.kwargs["recording_directory"], "captures")
        self.assertEqual(run.call_args.kwargs["context"]["diagnostic_context"], {"phase": "run"})
        self.assertEqual(terminal._native._context["diagnostic_context"], {"case": "save"})

    def test_finish_trace_interrupt_and_owned_results(self):
        terminal = TuiTest("parity")
        for name, argument in (("finish_trace", True), ("interrupt", None)):
            with patch.object(terminal._native, name, new_callable=AsyncMock) as method:
                asyncio.run(getattr(terminal, name)(argument) if argument is not None
                            else getattr(terminal, name)())
                method.assert_awaited_once()
        with patch.object(terminal._native, "packed_screen", new_callable=AsyncMock,
                          return_value=(memoryview(b"Save \n"), 5, 1)):
            data, cols, rows = asyncio.run(terminal.packed_screen(full=True))
            self.assertEqual((data.tobytes(), cols, rows), (b"Save \n", 5, 1))
        with patch.object(terminal._native, "highlight_locator", new_callable=AsyncMock,
                          return_value=[]) as highlight:
            self.assertEqual(asyncio.run(terminal.get_by_text("Save").highlight_matches()), [])
            highlight.assert_awaited_once()

    def test_explicit_empty_context_trace_disables_recording_and_trace_defaults(self):
        for method in ("open", "run"):
            for scope in ("constructor", "spawn", "absent"):
                with self.subTest(method=method, scope=scope):
                    context = ExecutionContext(trace=TraceOptions())
                    terminal = TuiTest(
                        "parity", trace=TraceOptions(mode="on"),
                        context=context if scope == "constructor" else None,
                    )
                    calls = []

                    def capture(name, arguments, kind):
                        calls.append(json.loads(_native._text(arguments[0])))
                        return {}

                    options = {"recording": AutomaticRecording(mode="always")}
                    if scope == "spawn":
                        options["context"] = context
                    arguments = ("example",) if method == "run" else ()
                    with patch.object(_native, "_invoke", side_effect=capture):
                        asyncio.run(getattr(terminal, method)(*arguments, **options))
                    self.assertEqual(calls[0]["trace_mode"],
                                     "on" if scope == "absent" else "off")
                    self.assertIsNone(context.trace.mode)
                    self.assertEqual(options["recording"].mode, "always")

    def test_spawn_recording_can_reset_the_constructor_directory(self):
        terminal = TuiTest("parity", recording=AutomaticRecording(directory="default", mode="on-failure"))
        directories = []

        def capture(name, arguments, kind):
            options = arguments[1]._obj
            directories.append((_native._text(options.recording_directory), _native._text(options.recording_mode)))
            return {}

        async def scenario():
            await terminal.run("example")
            await terminal.run("example", recording=AutomaticRecording(directory="override", mode="always"))
            await terminal.run("example", recording=AutomaticRecording())

        with patch.object(_native, "_invoke", side_effect=capture):
            asyncio.run(scenario())
        self.assertEqual(directories, [("default", "on-failure"), ("override", "always"), (None, None)])
        self.assertEqual(terminal._native._recording_directory, "default")


if __name__ == "__main__":
    unittest.main()
