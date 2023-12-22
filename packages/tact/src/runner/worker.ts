import process from "node:process";
import url from "node:url";
import childProcess from "node:child_process";
import module from "node:module";
import path from "node:path";

import { Suite } from "../suite.js";
import { TactTestOptions } from "../test/option.js";
import { spawn } from "../terminal/term.js";
import { defaultShell } from "../terminal/shell.js";

type WorkerResult = {
  stdout: string;
  stderr: string;
  exitCode?: number;
  error?: string;
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
  process.exit(0);
};

export function runTestWorker(testId: number, importPath: string, options: TactTestOptions, timeout: number): Promise<WorkerResult> {
  return new Promise((resolve, reject) => {
    const worker = childProcess.fork(__filename);
    worker.send(JSON.stringify({ testId, importPath, options }));
    let stdout = "";
    let stderr = "";
    worker.stdout?.on("data", (data) => (stdout += data));
    worker.stderr?.on("data", (data) => (stderr += data));

    const exitListener = (exitCode: number) => {
      if (exitCode !== 0) {
        resolve({
          stdout,
          stderr,
          passed: false,
          error: `Error: worker terminated with exit code ${exitCode}`,
          exitCode,
        });
      } else {
        resolve({ stderr, stdout, passed: true });
      }
    };
    worker.on("exit", exitListener);

    worker.on("message", (msg) => {
      worker.removeListener("exit", exitListener);
      const { errorMessage } = msg as { errorMessage: string };
      resolve({
        stdout,
        stderr,
        passed: false,
        error: errorMessage,
      });
    });
    worker.on("error", (error) => {
      worker.removeListener("exit", exitListener);
      resolve({
        stdout,
        stderr,
        passed: false,
        error: error.stack ?? error.message,
      });
    });

    setTimeout(async () => {
      worker.removeListener("exit", exitListener);
      worker.kill("SIGKILL");
      resolve({
        stderr,
        stdout,
        passed: false,
        error: `Error: worker was terminated as the timeout (${timeout} ms) as exceeded`,
      });
    }, timeout);
  });
}

// resolve if this file is the main module
const scriptPath = module.createRequire(import.meta.url).resolve(process.argv[1]);
const modulePath = path.extname(scriptPath) ? __filename : path.extname(__filename) ? __filename.slice(0, -path.extname(__filename).length) : __filename;
if (scriptPath === modulePath) {
  process.on("message", async (m: string) => {
    const { testId, importPath, options } = JSON.parse(m);
    try {
      await runTest(testId, importPath, options);
    } catch (e) {
      let errorMessage;
      if (typeof e == "string") {
        errorMessage = e;
      } else if (e instanceof Error) {
        errorMessage = e.stack ?? e.message;
      }
      if (errorMessage && process.send) {
        process.send({ errorMessage });
      }
    }
  });
}
