import path from "node:path";
import { glob } from "glob";
import { Suite } from "./suite.js";
import { transformFiles } from "./transform.js";
import { defaultShell } from "./shell.js";
import { spawn } from "./term.js";

declare global {
  var suite: Suite;
}

const streamBufferInitialSize = 100 * 1024; // 100 kb
const streamBufferIncrementAmount = 10 * 1024; // 10 kb

const collectSuites = async (matchPattern: string) => {
  const files = await glob(matchPattern);
  return files.map((file) => new Suite(file, "file"));
};

const runSuite = async (suite: Suite) => {
  for (const subsuite of suite.suites) {
    await runSuite(subsuite);
  }
  for (const test of suite.tests) {
    const { shell, rows, columns } = suite.options();
    const terminal = await spawn({ shell: shell ?? defaultShell, rows: rows ?? 30, cols: columns ?? 80 });
    let stderr = "";
    let stdout = "";
    const stdoutStream = process.stdout;
    const stderrStream = process.stdout;
    // TODO: actually capture the stdout/stderr
    try {
      await Promise.resolve(test.testFunction({ terminal }));
      test.passed = true;
    } catch (err: unknown) {
      if (typeof err === "string") {
        test.error = err;
      } else if (err instanceof Error) {
        test.error = err.message;
        test.errorStack = err.stack;
      }
      console.log(test.errorStack);
      test.passed = false;
    }
    test.stderr = stderr;
    test.stdout = stdout;
    terminal.kill();
  }
};

const run = async () => {
  const suites = await collectSuites("src/**/*.test.{ts,js}");
  await transformFiles();
  for (const suite of suites) {
    const transformedSuitePath = path.join(process.cwd(), ".tact", "cache", suite.name);
    const parsedSuitePath = path.parse(transformedSuitePath);
    const importablePath = `file://${path.join(parsedSuitePath.dir, `${parsedSuitePath.name}.js`)}`;
    globalThis.suite = suite;
    await import(importablePath);
  }
  for (const suite of suites) {
    await runSuite(suite);
  }
  process.exit(0);
};

await run();
