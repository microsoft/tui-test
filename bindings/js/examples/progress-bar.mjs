import { TuiTest } from "../dist/index.js";

const terminal = TuiTest.ephemeral("progress-bar");
try {
  await terminal.run(process.execPath, ["-e", `
    process.stdout.write("\\x1b]9;4;1;50\\x07");
    process.stdin.resume();
  `], { waitReady: false });
  await terminal.waitProgressBar({ state: "normal" });
  await terminal.expectProgressBar({ progress: 50 });
  console.log(await terminal.getProgressBar());
} finally {
  await terminal.closeQuiet();
}
