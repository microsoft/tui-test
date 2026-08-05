import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";

import {
  ExpectationError,
  InternalError,
  NoSessionError,
  ShellUse,
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

test("echo roundtrip drives a real session", async () => {
  await withTerminal({ shell }, async (su) => {
    await su.submit("echo hello-sdk");
    await su.waitCommand();
    await su.expectText("hello-sdk", { strict: false });
    await su.expectExitCode(0);
    const state = await su.state();
    assert.ok(state.cols > 0);
    assert.match(await su.text(), /hello-sdk/);
    assert.match(await su.getCommand(), /echo hello-sdk/);
    assert.match(await su.getOutput(), /hello-sdk/);
    assert.equal(await su.getExitCode(), 0);
    assert.equal(typeof (await su.getCwd()), "string");
    assert.deepEqual(await su.getCursor(), state.cursor);
    assert.deepEqual(await su.getSize(), { cols: state.cols, rows: state.rows });

    await su.resize(92, 26);
    assert.deepEqual(await su.getSize(), { cols: 92, rows: 26 });
    assert.ok((await su.cells(0, 0, 92, 26)).length > 0);
    assert.match(await su.screenshot(), /hello-sdk/);

    await su.write("echo typed-write");
    await su.keys("Enter");
    await su.waitText("typed-write");
    await su.waitCommand();
    await su.type("echo typed-type");
    await su.press("Enter");
    await su.waitText("typed-type");
    await su.waitCommand();
  });
});

test("bell state, waits, and expectations stay consistent", async () => {
  const su = ShellUse.ephemeral("bell-events");

  try {
    await su.open({ shell });
    await su.submit(twoBellsCommand);
    await su.expectBellCount(2, { timeout: 5000 });
    await su.waitCommand();

    assert.equal((await su.state()).bell_count, 2);
    assert.equal(await su.getBellCount(), 2);
    assert.equal(await su.getBellCount(), 2);

    await su.submit(delayedBellCommand);
    await su.waitBell({ timeout: 5000 });
    await su.expectBellCount(3);
    assert.equal(await su.getBellCount(), 3);
  } finally {
    await su.closeQuiet();
  }
});

test(
  "assertion errors include the current terminal",
  async () => {
    await withTerminal({ program: [process.execPath, ...evalArgs] }, async (su) => {
      await su.waitText("ready", { timeout: 2000 });
      await assert.rejects(
        su.expectText("text-that-is-not-on-screen", { timeout: 50 }),
        (error) =>
          error instanceof ExpectationError &&
          error.message.includes(
            "expectText: timed out after 50ms waiting for 'text-that-is-not-on-screen' to be visible",
          ) &&
          error.message.includes("Terminal content:\n╭") &&
          error.message.includes("ready") &&
          error.message.includes("\n╰"),
      );
      await assert.rejects(
        su.waitText("ready", { not: true, timeout: 50 }),
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
    await su.waitText("ready", { timeout: 2000 });

    const intervalMs = 10;
    const timeoutMs = 300;
    let ticks = 0;
    const heartbeat = setInterval(() => {
      ticks += 1;
    }, intervalMs);
    const start = Date.now();
    try {
      await assert.rejects(
        su.waitText("text-that-will-never-appear-xyz", { timeout: timeoutMs }),
        (error) => error instanceof ExpectationError,
      );
    } finally {
      clearInterval(heartbeat);
    }
    const elapsed = Date.now() - start;
    assert.ok(
      elapsed >= timeoutMs,
      `expected the wait to run for at least ${timeoutMs}ms, took ${elapsed}ms`,
    );
    const expectedTicks = Math.floor(timeoutMs / intervalMs);
    assert.ok(
      ticks >= expectedTicks * 0.5,
      `expected at least half of ~${expectedTicks} heartbeat ticks during the ` +
        `blocking wait, got ${ticks}; the event loop appears to have stalled`,
    );
  });
});

test("concurrent waits do not starve filesystem work", async () => {
  const root = mkdtempSync(join(tmpdir(), "shell-use-pool-"));
  const terminals = Array.from(
    { length: 6 },
    (_, index) => ShellUse.ephemeral(`pool-${index}`),
  );
  try {
    await Promise.all(
      terminals.map((terminal) => terminal.run(process.execPath, evalArgs)),
    );
    await Promise.all(
      terminals.map((terminal) => terminal.waitText("ready", { timeout: 2000 })),
    );

    const waitStart = Date.now();
    const waits = terminals.map((terminal) =>
      assert.rejects(
        terminal.waitText("text-that-will-never-appear-pool", { timeout: 800 }),
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
  const first = new ShellUse(name);
  const second = new ShellUse(name);
  try {
    await first.open({ shell });
    await second.submit("echo shared-native-session");
    await second.waitCommand();
    await first.waitText("shared-native-session");
    assert.match(await first.text(), /shared-native-session/);

    await second.resize(101, 27);
    assert.deepEqual(await first.getSize(), { cols: 101, rows: 27 });
  } finally {
    await first.closeQuiet();
    await second.closeQuiet();
  }
});

test("abandoning a raced promise keeps later operations serialized and safe", async () => {
  const su = new ShellUse(uniqueSession("promise-abandonment"));
  try {
    await su.run(process.execPath, evalArgs);
    await su.waitText("ready", { timeout: 2000 });

    const pending = su
      .waitText("never-visible-abandonment-marker", { timeout: 250 })
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
  const su = new ShellUse(name);
  const runtime = new NativeRuntime(name);
  const script =
    "process.stdout.write('\\x1b[31mI\\x1b[0m🙂\\x1b[38;2;1;2;3mR\\x1b[0m\\n');" +
    "setInterval(() => {}, 1000)";
  try {
    const args = typeof globalThis.Deno === "undefined" ? ["-e", script] : ["eval", script];
    const opened = await su.run(process.execPath, args);
    assert.ok(Object.hasOwn(opened, "shell_pid"));
    await su.waitText("R", { timeout: 2000 });
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

test("panic containment rejects as InternalError and Node keeps running", async () => {
  const runtime = new NativeRuntime(uniqueSession("panic-probe"));
  await assert.rejects(
    runtime.panicProbe(),
    (error) =>
      error instanceof InternalError &&
      error.message.includes("intentional native panic probe"),
  );

  try {
    await runtime.run({ program: process.execPath, args: evalArgs });
    await runtime.waitText("ready", { timeoutMs: 2000 });
    assert.match(await runtime.text(), /ready/);
  } finally {
    await runtime.close();
  }
});

test("typed mouse and signal operations execute against a real program", async () => {
  const su = new ShellUse(uniqueSession("typed-input-signal"));
  try {
    await su.run(process.execPath, evalArgs);
    await su.waitText("ready", { timeout: 2000 });
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

test("closeAll interrupts in-flight waits and closes every process-local session", async () => {
  const terminals = [
    new ShellUse(uniqueSession("close-all-a")),
    new ShellUse(uniqueSession("close-all-b")),
  ];
  await Promise.all(terminals.map((terminal) => terminal.run(process.execPath, evalArgs)));
  await Promise.all(
    terminals.map((terminal) => terminal.waitText("ready", { timeout: 2000 })),
  );

  const waiting = terminals[0]
    .waitText("never-visible-close-all-marker", { timeout: 30_000 })
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
  const su = new ShellUse(uniqueSession("nodetest"));
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
  const session = new ShellUse(name);
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
  const first = new ShellUse(name);
  const second = new ShellUse(name);
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
  const snapRoot = mkdtempSync(join(tmpdir(), "shell-use-snap-"));
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
