import process from "node:process";
import workerThreads from "node:worker_threads";
import url from "node:url";

import { Suite } from "./suite.js";
import { TactTestOptions } from "../testing/option.js";
import { spawn } from "./term.js";
import { defaultShell } from "./shell.js";

type WorkerResult = {
  stdout: string;
  stderr: string;
  exitCode?: number;
  error?: Error;
  passed: boolean;
};

const __filename = url.fileURLToPath(import.meta.url);

const runTest = async (testId: number, importPath: string, options: TactTestOptions) => {
  process.setSourceMapsEnabled(true);
  globalThis.suite = new Suite("runner-suite", "file");
  globalThis.tests = {};
  await import(importPath);
  const { shell, rows, columns } = options;
  const terminal = await spawn({ shell: shell ?? defaultShell, rows: rows ?? 30, cols: columns ?? 80 });
  await Promise.resolve(tests![testId].testFunction({ terminal }));
  terminal.kill();
};

export function runTestWorker(testId: number, importPath: string, options: TactTestOptions, timeout: number): Promise<WorkerResult> {
  return new Promise((resolve, reject) => {
    const worker = new workerThreads.Worker(__filename, {
      workerData: { testId, importPath, options },
      stderr: true,
      stdout: true,
    });
    let stdout = "";
    let stderr = "";
    worker.stdout.on("data", (data) => (stdout += data));
    worker.stderr.on("data", (data) => (stderr += data));

    const exitListener = (exitCode: number) => {
      if (exitCode !== 0)
        resolve({
          stdout,
          stderr,
          passed: false,
          error: new Error(`Error: worker terminated with exit code ${exitCode}`),
          exitCode,
        });
    };
    worker.on("message", () =>
      resolve({
        stdout,
        stderr,
        passed: true,
      })
    );
    worker.on("error", (error) =>
      resolve({
        stdout,
        stderr,
        passed: false,
        error,
      })
    );
    worker.on("exit", exitListener);
    setTimeout(async () => {
      worker.removeListener("exit", exitListener);
      await worker.terminate();
      resolve({
        stderr,
        stdout,
        passed: false,
        error: new Error(`Error: worker was terminated as the timeout (${timeout} ms) as exceeded`),
      });
    }, timeout);
  });
}

if (!workerThreads.isMainThread) {
  const { testId, importPath, options } = workerThreads.workerData as { testId: number; importPath: string; options: TactTestOptions };
  await runTest(testId, importPath, options);
  workerThreads.parentPort?.postMessage("passed");
}
