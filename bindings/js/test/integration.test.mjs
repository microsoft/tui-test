import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";

import {
  ExpectationError,
  InternalError,
  NoSessionError,
  TuiTest,
  closeAll,
  getRecording,
  sessions,
  uniqueSession,
} from "../dist/index.js";
import { NativeRuntime } from "../dist/native.js";
import { withTerminal } from "../dist/test/index.js";

const shell = process.platform === "win32" ? "pwsh" : undefined;
const evalArgs =
  typeof globalThis.Deno === "undefined"
    ? ["-e", "console.log('ready'); setInterval(() => {}, 1000)"]
    : ["eval", "console.log('ready'); setInterval(() => {}, 1000)"];
const twoBellsCommand =
  process.platform === "win32"
    ? "[Console]::Out.Write([char]7); [Console]::Out.Write([char]7)"
    : "printf '\\a\\a'";
const delayedBellCommand =
  process.platform === "win32"
    ? "Start-Sleep -Seconds 1; [Console]::Out.Write([char]7)"
    : "sleep 1; printf '\\a'";
const nonzeroExitArgs =
  typeof globalThis.Deno === "undefined"
    ? ["-e", "process.exit(7)"]
    : ["eval", "Deno.exit(7)"];

test("echo roundtrip drives a real session", async () => {
  await withTerminal({ shell }, async (su) => {
    await su.submit("echo hello-sdk");
    await su.waitCommand();
    await su.getByText("hello-sdk").first().expect();

    // A command finishes before the shell draws its next prompt. Wait for the
    // prompt-end marker so these separate cursor reads cannot straddle that
    // redraw and disagree by the prompt width.
    await su.waitReady();
    const state = await su.state();
    assert.deepEqual(await su.getCursor(), state.cursor);
    assert.ok(state.cols > 0);
    assert.match(await su.text(), /hello-sdk/);
    assert.equal(typeof (await su.getCwd()), "string");
    assert.deepEqual(await su.getSize(), { cols: state.cols, rows: state.rows });

    await su.resize(92, 26);
    assert.deepEqual(await su.getSize(), { cols: 92, rows: 26 });
    assert.ok((await su.cells(0, 0, 92, 26)).length > 0);
    assert.match(await su.screenshot(), /hello-sdk/);
    await assert.rejects(() => su.screenshot(null, { zoom: 0.5 }), /requires a path/);

    await su.write("echo typed-write");
    await su.keyboard.press("Enter");
    await su.getByText("typed-write").wait();
    await su.waitCommand();
    await su.type("echo typed-type");
    await su.keyboard.press("Enter");
    await su.getByText("typed-type").wait();
    await su.waitCommand();
  });
});

test("bell state, waits, and expectations stay consistent", async () => {
  const su = TuiTest.ephemeral("bell-events");

  try {
    await su.open({ shell });
    await su.submit(twoBellsCommand);
    await su.expectBellCount(2, { timeout: 5000 });
    await su.waitCommand();

    const initialState = await su.state();
    assert.equal(initialState.bell_count, 2);
    const initialEvents = await su.getBellEvents();
    assert.deepEqual(
      initialEvents.map((event) => event.sequence),
      [1, 2],
    );
    assert.ok(initialEvents[1].elapsed_ms >= initialEvents[0].elapsed_ms);
    assert.equal(await su.getBellCount(), 2);
    assert.equal(await su.getBellCount(), 2);

    await su.submit(delayedBellCommand);
    await su.waitBell({ timeout: 5000 });
    await su.expectBellCount(3);
    assert.equal(await su.getBellCount(), 3);
    const finalState = await su.state();
    assert.equal(finalState.bell_count, 3);
    const finalEvents = await su.getBellEvents();
    assert.deepEqual(
      finalEvents.map((event) => event.sequence),
      [1, 2, 3],
    );
    assert.ok(finalEvents[2].elapsed_ms >= finalEvents[1].elapsed_ms);
  } finally {
    await su.closeQuiet();
  }
});

test("recording API writes an asciicast file", async () => {
  const root = mkdtempSync(join(tmpdir(), "tui-test-recording-"));
  const path = join(root, "demo.cast");
  try {
    await withTerminal({ shell }, async (su) => {
      await su.startRecording(path, {
        format: "cast",
        fps: 24,
        speed: 1,
        idleTimeLimit: 2,
      });
      await su.submit("echo sdk-recording");
      await su.waitCommand();
      assert.equal(await su.stopRecording(), path);
    });
    const cast = await readFile(path, "utf8");
    assert.match(cast, /"version":2/);
    assert.match(cast, /sdk-recording/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("recording API exports styled Unicode to APNG and GIF", async () => {
  const root = mkdtempSync(join(tmpdir(), "tui-test-raster-recording-"));
  const command =
    process.platform === "win32"
      ? 'Write-Host "`e[1;3mstyled-é`e[0m"'
      : "printf '\\033[1;3mstyled-é\\033[0m\\n'";
  try {
    for (const [format, extension] of [
      ["apng", "png"],
      ["gif", "gif"],
    ]) {
      const path = join(root, `styled.${extension}`);
      await withTerminal({ shell, cols: 20, rows: 4 }, async (su) => {
        if (format === "apng") {
          const screenshotPath = join(root, "zoomed.svg");
          await su.screenshot(screenshotPath, { zoom: 0.5 });
          assert.match(
            await readFile(screenshotPath, "utf8"),
            /width="139" height="92" viewBox="0 0 278 184"/,
          );
        }
        await su.startRecording(path, { format, fps: 30, zoom: 0.5 });
        await su.submit(command);
        await su.waitCommand();
        assert.equal(await su.stopRecording(), path);
      });
      const bytes = await readFile(path);
      if (format === "apng") {
        assert.deepEqual(bytes.subarray(0, 8), Buffer.from("\x89PNG\r\n\x1a\n", "latin1"));
        assert.ok(bytes.includes(Buffer.from("acTL")));
        assert.equal(bytes.readUInt32BE(16), 278);
        assert.equal(bytes.readUInt32BE(20), 184);
      } else {
        assert.equal(bytes.subarray(0, 6).toString("ascii"), "GIF89a");
        assert.equal(bytes.readUInt16LE(6), 278);
        assert.equal(bytes.readUInt16LE(8), 184);
      }
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test(
  "assertion errors include the current terminal",
  async () => {
    await withTerminal({ program: [process.execPath, ...evalArgs] }, async (su) => {
      await su.getByText("ready").wait({ timeout: 2000 });
      await assert.rejects(
        su.getByText("text-that-is-not-on-screen").unique().expect({ timeout: 50 }),
        (error) =>
          error instanceof ExpectationError &&
          error.message.includes(
            "locator.expect: timed out after 50ms waiting for 'text-that-is-not-on-screen' to be visible",
          ) &&
          error.message.includes("Terminal content:\n╭") &&
          error.message.includes("ready") &&
          error.message.includes("\n╰"),
      );
      await assert.rejects(
        su.getByText("ready").wait({ state: "hidden", timeout: 50 }),
        (error) =>
          error instanceof ExpectationError &&
          error.message.includes("timed out after 50ms waiting for 'ready' to be hidden") &&
          error.message.includes("Terminal content:\n╭"),
      );
      await assert.rejects(
        su.expectOutput("missing"),
        (error) =>
          error instanceof ExpectationError &&
          error.message.includes("no command output tracked yet") &&
          error.message.includes("Terminal content:\n╭") &&
          error.message.includes("ready"),
      );
    });
  },
);

test("a blocking native wait runs off the JS event loop", async () => {
  await withTerminal({ program: [process.execPath, ...evalArgs] }, async (su) => {
    await su.getByText("ready").wait({ timeout: 2000 });

    const timeoutMs = 300;
    const eventLoopTurn = new Promise((resolve) =>
      setTimeout(() => resolve("event-loop"), 0),
    );
    const wait = su.getByText("text-that-will-never-appear-xyz").wait({
      timeout: timeoutMs,
    });
    const first = await Promise.race([
      eventLoopTurn,
      wait.then(
        () => "wait",
        () => "wait",
      ),
    ]);

    assert.equal(first, "event-loop", "the native wait blocked the event loop");
    await assert.rejects(wait, (error) => error instanceof ExpectationError);
  });
});

test("repeated direct runtime exits retain their nonzero status", async () => {
  for (let index = 0; index < 20; index += 1) {
    const su = TuiTest.ephemeral(`direct-nonzero-${index}`);
    try {
      await su.run(process.execPath, nonzeroExitArgs);
      await su.waitExit({ timeout: 5000 });
      assert.equal((await su.state()).exited, 7);
    } finally {
      await su.closeQuiet();
    }
  }
});

test("concurrent waits do not starve filesystem work", async () => {
  const root = mkdtempSync(join(tmpdir(), "tui-test-pool-"));
  const terminals = Array.from(
    { length: 6 },
    (_, index) => TuiTest.ephemeral(`pool-${index}`),
  );
  try {
    await Promise.all(
      terminals.map((terminal) => terminal.run(process.execPath, evalArgs)),
    );
    await Promise.all(
      terminals.map((terminal) => terminal.getByText("ready").wait({ timeout: 2000 })),
    );

    const waitStart = Date.now();
    const waits = terminals.map((terminal) =>
      assert.rejects(
        terminal.getByText("text-that-will-never-appear-pool").wait({ timeout: 800 }),
        (error) => error instanceof ExpectationError,
      ),
    );
    await new Promise((resolve) => setTimeout(resolve, 50));

    const start = Date.now();
    await writeFile(join(root, "probe.txt"), "ready");
    const elapsed = Date.now() - start;

    assert.ok(elapsed < 400, `filesystem work was delayed by ${elapsed}ms`);
    await Promise.all(waits);
    assert.ok(Date.now() - waitStart < 2500);
  } finally {
    await Promise.all(terminals.map((terminal) => terminal.closeQuiet()));
    rmSync(root, { recursive: true, force: true });
  }
});

test("same-name clients share one serialized native session", async () => {
  const name = uniqueSession("same-name");
  const first = new TuiTest(name);
  const second = new TuiTest(name);
  try {
    await first.open({ shell });
    await second.submit("echo shared-native-session");
    await second.waitCommand();
    await first.getByText("shared-native-session").wait();
    assert.match(await first.text(), /shared-native-session/);

    await second.resize(101, 27);
    assert.deepEqual(await first.getSize(), { cols: 101, rows: 27 });
  } finally {
    await first.closeQuiet();
    await second.closeQuiet();
  }
});

test("abandoning a raced promise keeps later operations serialized and safe", async () => {
  const su = new TuiTest(uniqueSession("promise-abandonment"));
  try {
    await su.run(process.execPath, evalArgs);
    await su.getByText("ready").wait({ timeout: 2000 });

    const pending = su
      .getByText("never-visible-abandonment-marker").wait({ timeout: 250 })
      .then(
        () => null,
        (error) => error,
      );
    const race = await Promise.race([
      pending.then(() => "completed"),
      new Promise((resolve) => setTimeout(() => resolve("abandoned"), 25)),
    ]);
    assert.equal(race, "abandoned");

    const start = Date.now();
    const laterState = su.state();
    const error = await pending;
    assert.ok(error instanceof ExpectationError);
    const state = await laterState;
    assert.ok(Date.now() - start >= 150);
    assert.ok(state.text.includes("ready"));
    assert.match(await su.text(), /ready/);
  } finally {
    await su.closeQuiet();
  }
});

test("private packed screens retain full UTF-8 logical rows and own their bytes", async () => {
  const name = uniqueSession("packed-screen");
  const su = new TuiTest(name);
  const runtime = new NativeRuntime(name);
  const script =
    "process.stdout.write('\\x1b[31mI\\x1b[0m🙂\\x1b[38;2;1;2;3mR\\x1b[0m\\n');" +
    "setInterval(() => {}, 1000)";
  try {
    const args = typeof globalThis.Deno === "undefined" ? ["-e", script] : ["eval", script];
    const opened = await su.run(process.execPath, args);
    assert.ok(Object.hasOwn(opened, "shell_pid"));
    await su.getByText("R").wait({ timeout: 2000 });
    assert.equal((await su.state()).session_shell, null);

    const cells = await su.cells(0, 0, 10, 2);
    const indexed = cells.find((cell) => cell.char === "I");
    const rgb = cells.find((cell) => cell.char === "R");
    assert.equal(indexed?.fg, 1);
    assert.equal(rgb?.fg, "#010203");

    const first = await runtime.packedScreen(false);
    assert.ok(first.utf8 instanceof Uint8Array);
    assert.ok(first.cols > 0 && first.rows > 0);
    assert.equal(
      Object.getOwnPropertyDescriptor(first, "utf8")?.writable,
      false,
    );
    const decoder = new TextDecoder();
    const encoder = new TextEncoder();
    const logicalRows = decoder.decode(first.utf8).split("\n");
    assert.equal(logicalRows.length, first.rows);
    assert.ok(logicalRows[0].startsWith("I🙂R"));
    assert.ok(logicalRows[0].endsWith(" "));
    assert.equal(logicalRows.at(-1), " ".repeat(first.cols));
    assert.ok(encoder.encode(logicalRows[0]).byteLength > first.cols);
    assert.notEqual(first.utf8.indexOf("R".charCodeAt(0)), rgb?.x);

    first.utf8.fill(0);
    const second = await runtime.packedScreen(false);
    assert.notStrictEqual(first.utf8, second.utf8);
    assert.match(decoder.decode(second.utf8), /I🙂R/);
    await su.close();
    assert.equal(first.utf8.length > 0, true);
  } finally {
    await su.closeQuiet();
  }
});

test("locators support scoped text matches and style assertions", async () => {
  const su = new TuiTest(uniqueSession("text-locators"));
  const script =
    "process.stdout.write('Settings One\\n  Save\\nSettings Two\\n  Save\\n\\x1b[1mWarning\\x1b[0m\\n\\x1b[1mPart\\x1b[0mial\\n');" +
    "setInterval(() => {}, 1000)";
  try {
    await su.run(process.execPath, ["-e", script]);
    await su.getByText("Warning").wait({ timeout: 2000 });
    const matches = await su
      .getByText("Settings")
      .getByText("Save", {
        whitespace: "normalize",
        direction: "after",
      })
      .locations();
    assert.equal(matches.length, 2);
    assert.equal(matches[0].start.row, 1);
    assert.equal(matches[0].start.column, 2);
    assert.equal(matches[1].start.row, 3);
    assert.equal(
      (
        await su
          .getByText("Save")
          .first()
          .getByText("Settings", { direction: "before" })
          .unique()
          .location()
      ).start.row,
      0,
    );
    assert.equal(
      (
        await su
          .getByStyle({ bold: true })
          .getByText("Save", { direction: "before" })
          .last()
          .location()
      ).start.row,
      3,
    );
    await su
      .getByText("Warning")
      .getByStyle({ bold: true })
      .unique()
      .expect();
    await assert.rejects(
      su
        .getByText("Warning")
        .getByStyle({ bold: false })
        .unique()
        .expect({ timeout: 20 }),
      (error) =>
        error instanceof ExpectationError &&
        error.message.includes("waiting for 'style' to be visible"),
    );
    await su
      .getByStyle({ bold: true })
      .getByText("Part")
      .unique()
      .expect();
    await assert.rejects(
      su
        .getByText("Partial")
        .getByStyle({ bold: true })
        .expect({ timeout: 20 }),
      (error) => error instanceof ExpectationError,
    );
  } finally {
    await su.closeQuiet();
  }
});

test("get-by locators are lazy, chainable, and actionable", async () => {
  const su = new TuiTest(uniqueSession("reusable-text-locators"));
  const script =
    typeof globalThis.Deno === "undefined"
      ? "setTimeout(() => process.stdout.write('item outside\\n\\x1b[1mitem item\\x1b[0m\\n'), 200); setInterval(() => {}, 1000)"
      : "setTimeout(() => Deno.stdout.writeSync(new TextEncoder().encode('item outside\\n\\x1b[1mitem item\\x1b[0m\\n')), 200); setInterval(() => {}, 1000)";
  const args = typeof globalThis.Deno === "undefined" ? ["-e", script] : ["eval", script];
  try {
    await su.run(process.execPath, args);
    const locator = su.getByText("item");
    assert.throws(() => locator.nth(-1), /non-negative integer/);
    assert.throws(
      () => su.getByText("item", { occurrence: "last" }),
      /select locator occurrences with/,
    );
    assert.throws(
      () => locator.expect({ style: { bold: true } }),
      /refine the locator with getByStyle/,
    );
    assert.throws(() => su.getByStyle({}), /at least one style/);

    const waited = await locator.wait({ timeout: 2000 });
    assert.strictEqual(waited, locator);
    assert.equal(await locator.count(), 3);
    await locator.any().expect({ timeout: 20 });
    await locator.expect({ timeout: 20 });
    await locator.first().expect({ timeout: 20 });
    await locator.last().expect({ timeout: 20 });
    await locator.nth(2).expect({ timeout: 20 });
    await locator.nth(3).expect({ not: true, timeout: 20 });
    await su
      .getByText("missing-item")
      .unique()
      .expect({ not: true, timeout: 20 });
    await assert.rejects(
      locator.unique().expect({ timeout: 20 }),
      (error) => error instanceof ExpectationError,
    );
    await assert.rejects(
      locator.unique().expect({ not: true, timeout: 20 }),
      (error) =>
        error instanceof ExpectationError &&
        error.message.includes("found 3"),
    );
    await assert.rejects(
      locator.first().expect({ not: true, timeout: 20 }),
      (error) => error instanceof ExpectationError,
    );
    const nested = su
      .getByText("item item")
      .getByStyle({ bold: true })
      .getByText("tem");
    await nested.wait({ timeout: 2000 });
    assert.equal(await nested.count(), 2);
    assert.equal(
      await su.getByStyle({ bold: true }).getByText("item").count(),
      2,
    );

    const items = await locator.all();
    assert.equal(items.length, 3);
    assert.equal((await items[0].location()).start.column, 0);
    assert.equal((await items[1].location()).start.row, 1);
    assert.equal((await locator.last().location()).start.column, 5);
    await assert.rejects(
      locator.unique().locations(),
      (error) => error instanceof ExpectationError,
    );
    await assert.rejects(
      su.getByText("missing-item").location(),
      (error) =>
        error instanceof ExpectationError &&
        error.message.includes("Terminal content:") &&
        error.message.includes("item item"),
    );

    await nested.highlight();
    await nested.first().click({ timeout: 2000 });
    await nested.first().expect();

    await su.getByText("item").wait({ timeout: 2000 });
    await su.getByText("item").first().highlight();
  } finally {
    await su.closeQuiet();
  }
});

test("panic containment rejects as InternalError and Node keeps running", async () => {
  const name = uniqueSession("panic-probe");
  const runtime = new NativeRuntime(name);
  const client = new TuiTest(name);
  await assert.rejects(
    runtime.panicProbe(),
    (error) =>
      error instanceof InternalError &&
      error.message.includes("intentional native panic probe"),
  );

  try {
    await runtime.run({ program: process.execPath, args: evalArgs });
    await client.getByText("ready").wait({ timeout: 2000 });
    assert.match(await runtime.text(), /ready/);
  } finally {
    await runtime.close();
  }
});

test("typed mouse and signal operations execute against a real program", async () => {
  const su = new TuiTest(uniqueSession("typed-input-signal"));
  try {
    await su.run(process.execPath, evalArgs);
    await su.getByText("ready").wait({ timeout: 2000 });
    await su.mouse.move(1, 1);
    await su.mouse.down(1, 1);
    await su.mouse.up(1, 1);
    await su.mouse.drag(1, 1, 2, 2);
    await su.mouse.scroll("down", { amount: 1 });
    await su.mouse.click(1, 1);
    await su.signal("KILL");
    assert.match(await su.text(), /ready/);
    await su.close();
    await assert.rejects(su.state(), (error) => error instanceof NoSessionError);
  } finally {
    await su.closeQuiet();
  }
});

test("ghostty backend preserves blink", async () => {
  const su = new TuiTest(uniqueSession("ghostty-blink"), {
    backend: "ghostty",
  });
  try {
    await su.run(
      process.execPath,
      ["-e", "process.stdout.write('\\u001b[5mX\\u001b[0m'); setInterval(() => {}, 1000)"],
      { cols: 10, rows: 2 },
    );
    await su.getByText("X").wait({ timeout: 5000 });
    assert.equal((await su.cells(0, 0))[0].blink, true);
  } finally {
    await su.closeQuiet();
  }
});

test("closeAll interrupts in-flight waits and closes every process-local session", async () => {
  const terminals = [
    new TuiTest(uniqueSession("close-all-a")),
    new TuiTest(uniqueSession("close-all-b")),
  ];
  await Promise.all(terminals.map((terminal) => terminal.run(process.execPath, evalArgs)));
  await Promise.all(
    terminals.map((terminal) => terminal.getByText("ready").wait({ timeout: 2000 })),
  );

  const waiting = terminals[0]
    .getByText("never-visible-close-all-marker").wait({ timeout: 30_000 })
    .then(
      () => null,
      (error) => error,
    );
  await new Promise((resolve) => setTimeout(resolve, 100));
  const start = Date.now();
  await closeAll();
  assert.ok(Date.now() - start < 2000);
  assert.ok((await waiting) instanceof ExpectationError);

  const open = await sessions();
  for (const terminal of terminals) {
    assert.ok(!open.includes(terminal.session));
    await assert.rejects(
      terminal.state(),
      (error) => error instanceof NoSessionError,
    );
  }
});

test("sessions lists an open session", async () => {
  const su = new TuiTest(uniqueSession("nodetest"));
  await su.open({ shell });
  try {
    const names = await sessions();
    assert.ok(names.includes(su.session));
  } finally {
    await su.closeQuiet();
  }
});

test("close evicts the session and retains its recording", async () => {
  const name = uniqueSession("recording");
  const session = new TuiTest(name);
  await session.open({ shell });
  await session.submit("echo retained-recording");
  await session.waitCommand();
  await session.close();

  assert.ok(!(await sessions()).includes(name));
  await assert.rejects(session.state(), (error) => error instanceof NoSessionError);
  assert.match(await getRecording(name), /retained-recording/);
  await assert.rejects(
    getRecording(uniqueSession("missing-recording")),
    (error) => error instanceof NoSessionError,
  );
});

test("any shared handle can close a reopened named session", async () => {
  const name = uniqueSession("shared-close");
  const first = new TuiTest(name);
  const second = new TuiTest(name);
  try {
    await first.open({ shell });
    await first.close();
    await second.open({ shell });
    await first.close();
    assert.ok(!(await sessions()).includes(name));
  } finally {
    await second.closeQuiet();
  }
});

test("snapshot lands in the client cwd", async () => {
  const snapRoot = mkdtempSync(join(tmpdir(), "tui-test-snap-"));
  const name = `snap-${basename(snapRoot)}`;
  const original = process.cwd();
  try {
    await withTerminal({ shell }, async (su) => {
      await su.submit("echo snapshot-marker");
      await su.waitCommand();
      await su.waitIdle();
      process.chdir(snapRoot);
      const status = await su.expectSnapshot(name);
      assert.equal(status, "written");
      assert.ok(existsSync(join(snapRoot, "__snapshots__", `${name}.snap`)));
      assert.ok(!existsSync(join(original, "__snapshots__", `${name}.snap`)));
      assert.equal(await su.expectSnapshot(name), "passed");
    });
  } finally {
    process.chdir(original);
    rmSync(snapRoot, { recursive: true, force: true });
  }
});
