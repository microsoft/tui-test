import { TuiTest } from "../dist/index.js";

const terminal = TuiTest.ephemeral("program-status");
try {
  await terminal.run(process.execPath, ["-e", `
    process.stdout.write("\\x1b]7501;state=working:app=demo\\x07");
    process.stdout.write("\\x1b]7501;state=blocked:id=build:kind=permission:progress=50:title=QnVpbGQ=\\x07");
    process.stdin.resume();
  `], { waitReady: false });
  await terminal.waitProgramStatus({ id: "build", state: "blocked" });
  await terminal.expectProgramStatus({ id: "build", app: "demo", title: "Build" });
  console.log(await terminal.getProgramStatuses());
  console.log("missing:", await terminal.getProgramStatus("missing"));
} finally {
  await terminal.closeQuiet();
}
