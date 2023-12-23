import path from "node:path";
import url from "node:url";
import workerpool from "workerpool";
import { Suite, TestMap, loadSuites } from "../suite.js";
import { transformFiles } from "../transform.js";
import { getRetries, getTimeout, loadConfig } from "../config/config.js";
import { runTestWorker } from "./worker.js";
import { Shell } from "../terminal/shell.js";
import { ListReporter } from "../reporter/list.js";
import { Reporter } from "../reporter/reporter.js";

declare global {
  var suite: Suite;
  var tests: TestMap | undefined;
}

const __dirname = path.dirname(url.fileURLToPath(import.meta.url));
const pool = workerpool.pool(path.join(__dirname, "worker.js"), { workerType: "process" });

const runSuites = async (allSuites: Suite[], reporter: Reporter) => {
  const tasks: Promise<any>[] = [];
  const suites = [...allSuites];
  while (suites.length != 0) {
    const suite = suites.shift();
    if (!suite) break;
    tasks.push(
      ...suite.tests.map(async (test) => {
        for (let i = 0; i < Math.max(0, getRetries()) + 1; i++) {
          const startTime = Date.now();
          reporter.startTest(test, suite);
          const { error, passed } = await runTestWorker(test.suiteId, suite.source!, suite.options(), getTimeout(), pool);
          const executionTime = Date.now() - startTime;
          test.results.push({
            passed,
            error,
            executionTime,
          });
          reporter.endTest(test, suite);
          if (passed) break;
        }
      })
    );
    suites.push(...suite.suites);
  }
  return Promise.all(tasks);
};

const getTestData = (allSuites: Suite[]): { totalTests: number; shells: Shell[] } => {
  let totalTests = 0;
  const suites = [...allSuites];
  const shells = new Set<Shell>();
  while (suites.length != 0) {
    const suite = suites.pop();
    totalTests += suite?.tests.length ?? 0;
    const shell = suite?.options().shell;
    if (shell != null && suite?.tests.length != 0) shells.add(shell);
    suites.push(...(suite?.suites ?? []));
  }
  return { totalTests, shells: Array.from(shells) };
};

export const run = async () => {
  await transformFiles();
  const config = await loadConfig();
  const baseSuites = await loadSuites(config);
  const reporter = new ListReporter();

  for (const baseSuite of baseSuites) {
    for (const suite of baseSuite.suites) {
      const transformedSuitePath = path.join(process.cwd(), ".tact", "cache", suite.name);
      const parsedSuitePath = path.parse(transformedSuitePath);
      const extension = parsedSuitePath.ext.startsWith(".m") ? ".mjs" : ".js";
      const importablePath = `file://${path.join(parsedSuitePath.dir, `${parsedSuitePath.name}${extension}`)}`;
      suite.source = importablePath;
      globalThis.suite = suite;
      await import(importablePath);
    }
  }

  const { totalTests, shells } = getTestData(baseSuites);
  await reporter.start(totalTests, shells);

  if (config.globalTimeout > 0) {
    setTimeout(() => {
      console.error(`Error: global timeout (${config.globalTimeout} ms) exceeded`);
      process.exit(1);
    }, config.globalTimeout);
  }

  await runSuites(baseSuites, reporter);
  try {
    await pool.terminate(true);
  } catch {}
  process.exit(0);
};
