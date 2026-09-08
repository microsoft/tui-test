import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

import { withTerminal } from "../dist/test/index.js";

if (!process.argv.includes("--inspect-failure")) {
  console.log("Opt-in fixture: build the binding, then run npm run inspect:failure.");
} else {
  await withTerminal({
    prefix: "manual-inspection",
    program: [
      process.execPath,
      "-e",
      [
        "console.log('READY: intentionally failing inspection fixture');",
        "process.stdin.setRawMode?.(true);",
        "process.stdin.on('data', data => process.stdout.write(data));",
        "setInterval(() => {}, 1000);",
      ].join(""),
    ],
    monitoring: {
      enabled: true,
      waitAtEnd: "failure",
      metadata: {
        testFile: fileURLToPath(import.meta.url),
        testName: "intentional failure for interactive inspection",
        framework: "manual",
        tags: ["manual", "intentionally-failing"],
      },
    },
  }, async (terminal) => {
    await terminal.getByText("READY: intentionally failing inspection fixture")
      .wait({ timeout: 10_000 });
    assert.equal("actual", "expected", "Intentional failure: inspect the still-running PTY");
  });
}
