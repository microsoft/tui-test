import path from "node:path";
import url from "node:url";
import os from "node:os";
import workerpool from "workerpool";

import { Suite, getRootSuite } from "../test/suite.js";
import { transformFiles } from "./transform.js";
import { getRetries, getTimeout, loadConfig } from "../config/config.js";
import { runTestWorker } from "./worker.js";
import { Shell } from "../terminal/shell.js";
import { ListReporter } from "../reporter/list.js";
import { BaseReporter } from "../reporter/base.js";
import { TestCase, TestResult } from "../test/testcase.js";

declare global {
  var suite: Suite;
  var tests: { [testId: string]: TestCase };
}

const __dirname = path.dirname(url.fileURLToPath(import.meta.url));
const maxWorkers = Math.max(os.cpus().length - 1, 1);
const pool = workerpool.pool(path.join(__dirname, "worker.js"), { workerType: "process", maxWorkers });

const runSuites = async (allSuites: Suite[], reporter: BaseReporter) => {
  const tasks: Promise<any>[] = [];
  const suites = [...allSuites];
  while (suites.length != 0) {
    const suite = suites.shift();
    if (!suite) break;
    tasks.push(
      ...suite.tests.map(async (test) => {
        for (let i = 0; i < Math.max(0, getRetries()) + 1; i++) {
          const testResult: TestResult = {
            status: "pending",
            duration: 0,
          };
          reporter.startTest(test, testResult);
          const { error, status, duration } = await runTestWorker(test, suite.source!, getTimeout(), pool);
          testResult.status = status;
          testResult.duration = duration;
          testResult.error = error;

          test.results.push(testResult);
          reporter.endTest(test, testResult);
          if (status == "expected" || status == "skipped") break;
        }
      })
    );
    suites.push(...suite.suites);
  }
  return Promise.all(tasks);
};

export const run = async () => {
  await transformFiles();
  const config = await loadConfig();
  const rootSuite = await getRootSuite(config);
  const reporter = new ListReporter();

  const suites = [rootSuite];
  while (suites.length != 0) {
    const importSuite = suites.shift();
    if (importSuite?.type === "file") {
      const transformedSuitePath = path.join(process.cwd(), ".tact", "cache", importSuite.title);
      const parsedSuitePath = path.parse(transformedSuitePath);
      const extension = parsedSuitePath.ext.startsWith(".m") ? ".mjs" : ".js";
      const importablePath = `file://${path.join(parsedSuitePath.dir, `${parsedSuitePath.name}${extension}`)}`;
      importSuite.source = importablePath;
      globalThis.suite = importSuite;
      await import(importablePath);
    }
    suites.push(...(importSuite?.suites ?? []));
  }

  const allTests = rootSuite.allTests();
  const shells = Array.from(new Set(allTests.map((t) => t.suite.options?.shell).filter((s): s is Shell => s != null)));
  await reporter.start(allTests.length, shells);

  if (config.globalTimeout > 0) {
    setTimeout(() => {
      console.error(`Error: global timeout (${config.globalTimeout} ms) exceeded`);
      process.exit(1);
    }, config.globalTimeout);
  }

  await runSuites(rootSuite.suites, reporter);
  try {
    await pool.terminate(true);
  } catch {}
  const failures = reporter.end(rootSuite);
  process.exit(failures);
};
