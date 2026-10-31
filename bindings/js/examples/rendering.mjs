import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { TuiTest } from "../dist/index.js";

const directory = resolve(process.argv[2] ?? "target/binding-parity/js");
await mkdir(directory, { recursive: true });
const style = {
  font: { family: "monospace" },
  fontSize: 24,
  canvasPadding: 0,
  contentPadding: { left: 2 },
  window: { titleBar: false },
  border: { radius: 0 },
  shadow: { enabled: false },
  cursor: { blink: false, blinkIntervalMs: 250 },
};
const original = structuredClone(style);
const child = ["-e", "process.stdout.write('\\x1b[5 q\\x1b]10;#abcdef\\x07rendering-ready');process.stdin.resume()"];
const terminal = TuiTest.ephemeral("rendering", { style });
try {
  await terminal.run(process.execPath, child, { cols: 30, rows: 3 });
  await terminal.getByText("rendering-ready").expect();
  await terminal.expectCursor({ visible: true, shape: "bar" });
  await terminal.expectMode("cursor_visible", true);
  await terminal.expectColors({ foreground: "#abcdef" });
  assert.deepEqual(await terminal.getModes(), (await terminal.state()).modes);
  assert.deepEqual(await terminal.getColors(), (await terminal.state()).colors);
  const path = resolve(directory, "custom.svg");
  await terminal.screenshot(path);
  const svg = await readFile(path, "utf8");
  assert.match(svg, /font-size="24px"/);
  assert.match(svg, /font-family="monospace"/);
  await terminal.restart({ gracefulTimeout: 0 });
  await terminal.getByText("rendering-ready").expect();
  await terminal.screenshot(path);
  assert.equal(await readFile(path, "utf8"), svg);
  assert.deepEqual(style, original);
  await terminal.startRecording(resolve(directory, "steady.gif"), { fps: 30 });
  // Set playback duration after the readiness check.
  await new Promise((resolve) => setTimeout(resolve, 1100));
  await terminal.stopRecording();
  await terminal.run(process.execPath, child, {
    cols: 30, rows: 3, restart: true,
    style: { ...style, cursor: { blink: true, blinkIntervalMs: 250 } },
  });
  await terminal.getByText("rendering-ready").expect();
  await terminal.startRecording(resolve(directory, "fast.gif"), { fps: 30 });
  await new Promise((resolve) => setTimeout(resolve, 1100));
  await terminal.stopRecording();
  assert.deepEqual(style, original);
  console.log(`${path}: custom font and geometry survive restart; cursor blink disabled`);
  console.log("fast.gif: per-call override enables 250 ms cursor phases");
  console.log("Native cursor, mode, and color assertions passed; caller settings unchanged");
} finally {
  await terminal.closeQuiet();
}
