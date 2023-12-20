import path from "node:path";
import { glob } from "glob";
import { Suite, TestMap } from "./suite.js";
import { transformFiles } from "./transform.js";
import { runTestWorker } from "./worker.js";

declare global {
  var suite: Suite;
  var tests: TestMap | undefined;
}

const collectSuites = async (matchPattern: string) => {
  const files = await glob(matchPattern);
  return files.map((file) => new Suite(file, "file"));
};

const runSuite = async (suite: Suite) => {
  for (const subsuite of suite.suites) {
    await runSuite(subsuite);
  }
  for (const test of suite.tests) {
    const { stderr, stdout, error, passed } = await runTestWorker(test.id, suite.source!, suite.options(), 10_000); // TODO: remove and replace with test level timeout when config support added;
    test.passed = passed;
    test.stderr = stderr;
    test.stdout = stdout;
    if (!passed && error != null) {
      test.errorStack = error.stack;
    }
  }
};

const run = async () => {
  const suites = await collectSuites("src/**/*.test.{ts,js}");
  await transformFiles();
  for (const suite of suites) {
    const transformedSuitePath = path.join(process.cwd(), ".tact", "cache", suite.name);
    const parsedSuitePath = path.parse(transformedSuitePath);
    const importablePath = `file://${path.join(parsedSuitePath.dir, `${parsedSuitePath.name}.js`)}`;
    suite.source = importablePath;
    globalThis.suite = suite;
    await import(importablePath);
  }
  for (const suite of suites) {
    await runSuite(suite);
  }
  process.exit(0);
};

await run();
