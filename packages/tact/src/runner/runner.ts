import path from "node:path";
import { Suite, TestMap, loadSuites } from "../suite.js";
import { transformFiles } from "../transform.js";
import { runTestWorker } from "./worker.js";
import { getRetries, getTimeout, loadConfig } from "../config/config.js";

declare global {
  var suite: Suite;
  var tests: TestMap | undefined;
}

const runSuite = async (suite: Suite) => {
  for (const subsuite of suite.suites) {
    await runSuite(subsuite);
  }
  for (const test of suite.tests) {
    for (let i = 0; i < Math.min(0, getRetries()) + 1; i++) {
      const { stderr, stdout, error, passed } = await runTestWorker(test.id, suite.source!, suite.options(), getTimeout());
      const errorMessage = !passed && error != null ? error?.stack ?? error?.message ?? "Error: test failed, view stderr for more information" : undefined;
      test.results.push({
        passed,
        stderr,
        stdout,
        error: errorMessage,
      });
      if (passed) break;
    }
  }
};

const run = async () => {
  await transformFiles();
  const config = await loadConfig();
  const baseSuites = await loadSuites(config);
  for (const baseSuite of baseSuites) {
    for (const suite of baseSuite.suites) {
      const transformedSuitePath = path.join(process.cwd(), ".tact", "cache", suite.name);
      const parsedSuitePath = path.parse(transformedSuitePath);
      const importablePath = `file://${path.join(parsedSuitePath.dir, `${parsedSuitePath.name}.js`)}`;
      suite.source = importablePath;
      globalThis.suite = suite;
      await import(importablePath);
    }
  }
  if (config.globalTimeout > 0) {
    setTimeout(() => {
      console.error(`Error: global timeout (${config.globalTimeout} ms) exceeded`);
      process.exit(1);
    }, config.globalTimeout);
  }

  for (const suite of baseSuites) {
    await runSuite(suite);
  }
  process.exit(0);
};

// await run();
