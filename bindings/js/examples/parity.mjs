import assert from "node:assert/strict";
import { getRecording, TuiTest } from "../dist/index.js";

const directory = process.env.TUI_TEST_PARITY_ARTIFACTS ?? "artifacts/parity-js";
const terminal = TuiTest.ephemeral("parity", {
  context: { diagnosticContext: { example: "session-parity" } },
});
try {
  await terminal.run(process.execPath, ["-e", `
    process.stdout.write("Start\\r\\n\\x1b[1mSave\\x1b[0m\\r\\nEnd\\r\\n");
    process.stdin.resume();
  `], {
    waitReady: false,
    recording: { directory },
    context: {
      diagnosticContext: { phase: "spawn" },
      trace: { mode: "on", directory },
    },
  });
  const save = terminal.getByText("Save", {
    scope: { after: { text: "Start" }, before: { text: "End" } },
  }).withStyle({ bold: true });
  await save.expect();
  const composite = save.relativeTo(terminal.getByText("Start"), "after")
    .withStyle({ bold: true }).or(terminal.getByLink("test:save"));
  assert.equal(await composite.count(), 1);
  const matches = await composite.highlightMatches();
  const screen = await terminal.packedScreen();
  assert.equal(matches[0].text, "Save");
  assert.ok(new TextDecoder().decode(screen.utf8).includes("Save"));
  const snapshot = await terminal.expectSnapshot("parity", { cwd: directory, update: true });
  await terminal.finishTrace(false);
  await terminal.state();
  console.log(`scoped bold matches=${matches.length}; packed=${screen.cols}x${screen.rows}; snapshot=${snapshot}; trace finished; session still open`);
  await terminal.interrupt();
  console.log("pending operations cancelled; child terminated");
  await terminal.close();
  const cast = await terminal.run(process.execPath, ["-e", "console.log('cast-only'); process.stdin.resume();"], {
    waitReady: false, recording: { mode: "always", directory: `${directory}/cast-only` },
  });
  await terminal.getByText("cast-only").expect();
  assert.ok((await getRecording(terminal.session)).includes("cast-only"));
  console.log(`cast-only recording=${cast.recording}`);
} finally {
  await terminal.closeQuiet();
}
