import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";

import {
  ExpectationError,
  NoSessionError,
  ShellUse,
  UsageError,
  getRecording,
  sessions,
  uniqueSession,
} from "../dist/index.js";
import { withTerminal } from "../dist/test/index.js";

const shell = process.platform === "win32" ? "pwsh" : undefined;
const evalArgs =
  typeof globalThis.Deno === "undefined"
    ? ["-e", "console.log('ready'); setInterval(() => {}, 1000)"]
    : ["eval", "console.log('ready'); setInterval(() => {}, 1000)"];

test("echo roundtrip drives a real session", async () => {
  await withTerminal({ shell }, async (su) => {
    await su.submit("echo hello-sdk");
    await su.waitCommand();
    await su.expectText("hello-sdk", { strict: false });
    await su.expectExitCode(0);
    const state = await su.state();
    assert.ok(state.cols > 0);
  });
});

test("cli control requests are rejected by native sessions", async () => {
  await withTerminal({ shell }, async (session) => {
    await assert.rejects(
      session.send({ kind: "shutdown" }),
      (error) => error instanceof UsageError,
    );
    assert.ok((await session.state()).cols > 0);
  });
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
  await assert.rejects(
    session.send({ kind: "shutdown" }),
    (error) => error instanceof UsageError,
  );
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
