import process from "node:process";
import workerpool from "workerpool";

import { Suite, Test } from "../suite.js";
import { TactTestOptions } from "../test/option.js";
import { spawn } from "../terminal/term.js";
import { defaultShell } from "../terminal/shell.js";

type WorkerResult = {
  exitCode?: number;
  error?: string;
  passed: boolean;
};

const cache: { [importablePath: string]: { [testId: number]: Test } } = {};

const updateWorkersCache = (importablePath: string) => (test: Test) => {
  cache[importablePath] = { ...cache[importablePath], [test.id]: test };
};

const runTest = async (testId: number, importPath: string, options: TactTestOptions) => {
  process.setSourceMapsEnabled(true);
  globalThis.suite = new Suite("runner-suite", "file");
  globalThis.tests = {};
  globalThis.updateWorkerCache = updateWorkersCache(importPath);
  if (cache[importPath] == null) {
    await import(importPath);
  }
  const test = cache[importPath][testId];
  const { shell, rows, columns } = options;
  const terminal = await spawn({ shell: shell ?? defaultShell, rows: rows ?? 30, cols: columns ?? 80 });
  await Promise.resolve(test.testFunction({ terminal }));
};

export function runTestWorker(testId: number, importPath: string, options: TactTestOptions, timeout: number, pool: workerpool.Pool): Promise<WorkerResult> {
  return new Promise(async (resolve, reject) => {
    try {
      const poolPromise = pool.exec("testWorker", [testId, importPath, options], {
        on: (payload) => {
          if (payload.errorMessage) {
            resolve({
              passed: false,
              error: payload.errorMessage,
            });
          }
        },
      });
      if (timeout > 0) {
        poolPromise.timeout(timeout);
      }
      await poolPromise;
      resolve({
        passed: true,
      });
    } catch (e) {
      if (typeof e === "string") {
        resolve({
          passed: false,
          error: e,
        });
      } else if (e instanceof workerpool.Promise.TimeoutError) {
        resolve({
          passed: false,
          error: `Error: worker was terminated as the timeout (${timeout} ms) as exceeded`,
        });
      } else if (e instanceof Error) {
        resolve({
          passed: false,
          error: e.stack ?? e.message,
        });
      }
    }
  });
}

const testWorker = async (testId: number, importPath: string, options: TactTestOptions) => {
  try {
    await runTest(testId, importPath, options);
  } catch (e) {
    let errorMessage;
    if (typeof e == "string") {
      errorMessage = e;
    } else if (e instanceof Error) {
      errorMessage = e.stack ?? e.message;
    }
    if (errorMessage) {
      workerpool.workerEmit({
        errorMessage,
      });
    }
  }
};

if (!workerpool.isMainThread) {
  workerpool.worker({
    testWorker: testWorker,
  });
}
