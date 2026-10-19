import assert from "node:assert/strict";
import { test } from "node:test";
import { TuiTest, UsageError, NoSessionError } from "../dist/index.js";
import { NativeSession } from "../native/index.js";

test("native scroll validates signed integer amounts", async () => {
  const terminal = TuiTest.ephemeral();
  const native = new NativeSession(`${terminal.session}-native`);
  for (const amount of [1.5, NaN, Infinity, -Infinity, 2147483648, -2147483649, "1", true]) {
    await assert.rejects(terminal.scroll(amount), UsageError);
    await assert.rejects(async () => native.scroll(amount));
  }
  for (const amount of [0, 1, -1, -2147483648, 2147483647]) {
    await assert.rejects(terminal.scroll(amount), NoSessionError);
  }
});

test("native history view is independent of full text", async () => {
  const terminal = TuiTest.ephemeral(undefined, { backend: "alacritty" });
  try {
    await terminal.run(process.execPath, [typeof globalThis.Deno === "undefined" ? "-e" : "eval",
      "for(let i=0;i<40;i++)console.log('history-'+i);setInterval(()=>{},1000)",
    ], { rows: 5, cols: 40 });
    await terminal.getByText("history-39").expect();
    const full = await terminal.text({ full: true });
    const live = await terminal.text();
    const bottom = await terminal.getViewport();
    assert.equal(bottom.offset, 0);
    assert.ok(bottom.history > 0);
    await terminal.scroll(2);
    assert.equal((await terminal.getViewport()).offset, 2);
    await terminal.scroll(-1);
    assert.equal((await terminal.getViewport()).offset, 1);
    await terminal.scrollToTop();
    const top = await terminal.getViewport();
    assert.equal(top.offset, top.history);
    assert.match(await terminal.text(), /history-0/);
    assert.equal(await terminal.text({ full: true }), full);
    await assert.rejects(terminal.getByText("history-0").click());
    await terminal.scrollToBottom();
    assert.equal(await terminal.text(), live);
    await terminal.scrollToTop();
    await terminal.keyboard.press("a");
    assert.equal((await terminal.getViewport()).offset, 0);
  } finally {
    await terminal.closeQuiet();
  }
});
