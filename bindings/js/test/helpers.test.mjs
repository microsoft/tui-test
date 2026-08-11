import assert from "node:assert/strict";
import { test } from "node:test";

import { uniqueSession } from "../dist/index.js";
import {
  closeAllTracked,
  createTerminal,
  defaultShell,
  terminalSnapshot,
  trackTerminal,
  trackedCount,
  untrackTerminal,
  withTerminal,
} from "../dist/test/index.js";

test("terminalSnapshot trims trailing whitespace per line", () => {
  assert.equal(terminalSnapshot("a  \nb\t\nc"), "a\nb\nc");
});

test("terminalSnapshot drops trailing blank lines but keeps leading/interior", () => {
  assert.equal(terminalSnapshot("\nhi\n\nbye\n   \n\n"), "\nhi\n\nbye");
  assert.equal(terminalSnapshot("only   \n\n\n"), "only");
  assert.equal(terminalSnapshot(""), "");
  assert.equal(terminalSnapshot("\n\n"), "");
});

test("terminalSnapshot normalises carriage returns at line ends", () => {
  assert.equal(terminalSnapshot("a\r\nb\r\n"), "a\nb");
});

test("defaultShell is platform-aware", () => {
  if (process.platform === "win32") {
    assert.equal(defaultShell, "powershell");
  } else if (process.platform === "darwin") {
    assert.equal(defaultShell, "zsh");
  } else {
    assert.equal(defaultShell, "bash");
  }
});

test("uniqueSession has the documented shape", () => {
  const name = uniqueSession();
  assert.match(name, /^tui-test-\d+-[a-z0-9]+-\d+$/);
});

test("uniqueSession sanitizes unsafe characters", () => {
  const name = uniqueSession("a b/c!");
  assert.match(name, /^a-b-c-/);
  assert.match(name, /^[A-Za-z0-9_-]+$/);
});

test("uniqueSession is unique across calls and capped at 64 chars", () => {
  assert.notEqual(uniqueSession(), uniqueSession());
  const long = uniqueSession("x".repeat(200));
  assert.ok(long.length <= 64, `expected <= 64, got ${long.length}`);
  assert.match(long, /^[A-Za-z0-9_-]+$/);
});

test("closeAllTracked closes and forgets every tracked terminal", async () => {
  await closeAllTracked();
  assert.equal(trackedCount(), 0);
  const closed = [];
  const makeStub = (id) => ({
    async closeQuiet() {
      closed.push(id);
    },
  });
  const a = makeStub("a");
  const b = makeStub("b");
  trackTerminal(a);
  trackTerminal(b);
  assert.equal(trackedCount(), 2);
  await closeAllTracked();
  assert.deepEqual(closed.sort(), ["a", "b"]);
  assert.equal(trackedCount(), 0);
});

test("untrackTerminal removes a terminal from the registry", async () => {
  await closeAllTracked();
  const stub = {
    async closeQuiet() {
      throw new Error("should not be called");
    },
  };
  trackTerminal(stub);
  assert.equal(trackedCount(), 1);
  untrackTerminal(stub);
  assert.equal(trackedCount(), 0);
  await closeAllTracked();
});

test(
  "createTerminal + withTerminal drive a real shell",
  async () => {
    await closeAllTracked();
    const marker = `helper-e2e-${process.pid}`;
    const result = await withTerminal({ prefix: "helpers-e2e" }, async (terminal) => {
      await terminal.submit(`echo ${marker}`);
      await terminal.waitCommand();
      await terminal.expectText(marker, { strict: false });
      await terminal.expectExitCode(0);
      return "done";
    });
    assert.equal(result, "done");
    assert.equal(trackedCount(), 0);
  },
);

test(
  "createTerminal registers the terminal for automatic cleanup",
  async () => {
    await closeAllTracked();
    const terminal = await createTerminal({ prefix: "helpers-track" });
    try {
      assert.equal(trackedCount(), 1);
      await terminal.submit("echo tracked-cleanup");
      await terminal.waitCommand();
    } finally {
      await closeAllTracked();
    }
    assert.equal(trackedCount(), 0);
  },
);

test(
  "createTerminal can run a raw program",
  async () => {
    await closeAllTracked();
    const evalArgs =
      typeof globalThis.Deno === "undefined"
        ? ["-e", "console.log('prog-ready'); setInterval(() => {}, 1000)"]
        : ["eval", "console.log('prog-ready'); setInterval(() => {}, 1000)"];
    await withTerminal(
      { prefix: "helpers-prog", program: [process.execPath, ...evalArgs] },
      async (terminal) => {
        await terminal.waitText("prog-ready", { timeout: 5000 });
      },
    );
    assert.equal(trackedCount(), 0);
  },
);
