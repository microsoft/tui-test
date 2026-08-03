import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";

import { ExpectationError, ShellUse, sessions, uniqueSession } from "../dist/index.js";
import { withTerminal } from "../dist/test/index.js";

const BIN = process.env.SHELL_USE_BIN;
const skip = !BIN;
const shell = process.platform === "win32" ? "pwsh" : undefined;
const evalArgs =
  typeof globalThis.Deno === "undefined"
    ? ["-e", "console.log('ready'); setInterval(() => {}, 1000)"]
    : ["eval", "console.log('ready'); setInterval(() => {}, 1000)"];

test("echo roundtrip drives a real session", { skip }, async () => {
  await withTerminal({ shell }, async (su) => {
    await su.submit("echo hello-sdk");
    await su.waitCommand();
    await su.expectText("hello-sdk", { strict: false });
    await su.expectExitCode(0);
    const state = await su.state();
    assert.ok(state.cols > 0);
  });
});

test(
  "assertion errors include the current terminal",
  { skip },
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

test("sessions lists an open session", { skip }, async () => {
  const su = new ShellUse(uniqueSession("nodetest"));
  await su.open({ shell });
  try {
    const names = await sessions();
    assert.ok(names.includes(su.session));
  } finally {
    await su.closeQuiet();
  }
});

test("snapshot lands in the client cwd", { skip }, async () => {
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
