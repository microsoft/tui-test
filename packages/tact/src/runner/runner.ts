import path from "node:path";
import url from "node:url";
import os from "node:os";
import workerpool from "workerpool";
import { Suite, TestMap, loadSuites } from "../suite.js";
import { transformFiles } from "../transform.js";
import { getRetries, getTimeout, loadConfig } from "../config/config.js";
import { runTestWorker } from "./worker.js";

declare global {
  var suite: Suite;
  var tests: TestMap | undefined;
}

const __dirname = path.dirname(url.fileURLToPath(import.meta.url));
const pool = workerpool.pool(path.join(__dirname, "worker.js"), { workerType: "process" });

const runSuites = async (allSuites: Suite[]) => {
  const tasks: Promise<any>[] = [];
  const suites = [...allSuites];
  while (suites.length != 0) {
    const suite = suites.shift();
    if (!suite) break;
    tasks.push(
      ...suite.tests.map(async (test) => {
        for (let i = 0; i < Math.min(0, getRetries()) + 1; i++) {
          const { error, passed } = await runTestWorker(test.id, suite.source!, suite.options(), getTimeout(), pool);
          test.results.push({
            passed,
            error,
          });
          if (passed) break;
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
  const baseSuites = await loadSuites(config);
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
  if (config.globalTimeout > 0) {
    setTimeout(() => {
      console.error(`Error: global timeout (${config.globalTimeout} ms) exceeded`);
      process.exit(1);
    }, config.globalTimeout);
  }

  await runSuites(baseSuites);
  try {
    await pool.terminate(true);
  } catch {}
  process.exit(0);
};
