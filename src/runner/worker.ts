// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import process from "node:process";
import { EventEmitter } from "node:events";
import workerpool from "workerpool";

import { Suite } from "../test/suite.js";
import { spawn } from "../terminal/term.js";
import { defaultShell } from "../terminal/shell.js";
import { Snapshot, TestCase, TestStatus } from "../test/testcase.js";
import { expect } from "../test/test.js";
import { BaseReporter } from "../reporter/base.js";
import { poll } from "../utils/poll.js";
import { flushSnapshotExecutionCache } from "../test/matchers/toMatchSnapshot.js";
import { saveTrace, TracePoint } from "../trace/tracer.js";

type WorkerResult = {
  error?: string;
  stdout?: string;
  stderr?: string;
  status: TestStatus;
  duration: number;
  snapshots: Snapshot[];
};

type WorkerExecutionOptions = {
  timeout: number;
  updateSnapshot: boolean;
  shellReadyTimeout: number;
};

const importSet = new Set<string>();
const activeSuites: Suite[] = [];
const beforeAllExecuted = new Set<Suite>();

const runTest = async (
  testId: string,
  testSuite: Suite,
  updateSnapshot: boolean,
  trace: boolean,
  tracePoints: TracePoint[],
  importPath: string,
  shellReadyTimeout: number
) => {
  process.setSourceMapsEnabled(true);
  globalThis.suite = Suite.from(testSuite);
  globalThis.tests = globalThis.tests ?? {};
  globalThis.__expectState = { updateSnapshot };
  if (!importSet.has(importPath)) {
    await import(importPath);
    importSet.add(importPath);
  }
  const test = globalThis.tests[testId];

  const ancestry = test.suite.parentSuites();
  const { exit, enter } = Suite.computeTransition(activeSuites, ancestry);

  const exitHooks = exit.flatMap((s) => s.afterAllHooks);
  for (const hook of exitHooks) {
    await Promise.resolve(hook());
  }
  activeSuites.length -= exit.length;

  const enterHooks = enter
    .filter((s) => !beforeAllExecuted.has(s))
    .flatMap((s) => {
      beforeAllExecuted.add(s);
      return s.beforeAllHooks;
    });

  for (const hook of enterHooks) {
    await Promise.resolve(hook());
  }
  activeSuites.push(...enter);

  const { shell, rows, columns, env, program } = test.suite.options ?? {};
  const traceEmitter = new EventEmitter();
  traceEmitter.on("data", (data: string, time: number) =>
    tracePoints.push({ data, time })
  );
  traceEmitter.on("size", (rows: number, cols: number) =>
    tracePoints.push({ rows, cols })
  );
  const terminal = await spawn(
    {
      shell: shell ?? defaultShell,
      rows: rows ?? 30,
      cols: columns ?? 80,
      env,
      program,
    },
    trace,
    traceEmitter
  );

  // add slight delay for node-pty teardown
  let programExited: Promise<void> | undefined;
  if (program != null) {
    programExited = new Promise<void>((resolve) => {
      terminal.onExit(() => setTimeout(resolve, 100));
    });
  }

  const allTests = Object.values(globalThis.tests);
  const testPath = test.filePath();
  const testSignature = test.titlePath().slice(1).join(" › ");
  const signatureIdenticalTests = allTests.filter(
    (t) =>
      t.filePath() === testPath &&
      t.titlePath().slice(1).join(" › ") === testSignature
  );
  const signatureIdx = signatureIdenticalTests.findIndex(
    (t) => t.id == test.id
  );
  const currentConcurrentTestName = () =>
    `${testSignature} | ${signatureIdx + 1}`;

  expect.setState({
    ...expect.getState(),
    testPath,
    currentTestName: test.title,
    currentConcurrentTestName,
  });

  // wait on the shell to be ready with the prompt
  if (program == null) {
    const shellReady = await poll(
      () => {
        const view = terminal
          .getViewableBuffer()
          .map((row) => row.join(""))
          .join("\n");
        return view.includes(">  ");
      },
      50,
      shellReadyTimeout
    );
    if (!shellReady) {
      try {
        terminal.kill();
      } catch {
        // ignore
      }
      throw new Error(
        `shell readiness timeout: the shell prompt was not detected within ${shellReadyTimeout / 1000}s`
      );
    }
  } else if (programExited) {
    await Promise.race([
      programExited,
      new Promise<void>((r) => setTimeout(r, 5_000)),
    ]);
  }

  const suites = test.suite.parentSuites();
  try {
    for (const s of suites) {
      for (const hook of s.beforeEachHooks) {
        await Promise.resolve(hook({ terminal }));
      }
    }

    await Promise.resolve(test.testFunction({ terminal }));
  } finally {
    try {
      for (const s of suites) {
        for (const hook of s.afterEachHooks) {
          await Promise.resolve(hook({ terminal }));
        }
      }
    } finally {
      try {
        terminal.kill();
      } catch {
        // terminal can pre-terminate if program is provided
      }
    }
  }
};

export async function runTestWorker(
  test: TestCase,
  importPath: string,
  { timeout, updateSnapshot, shellReadyTimeout }: WorkerExecutionOptions,
  trace: boolean,
  pool: workerpool.Pool,
  reporter: BaseReporter,
  attempt: number,
  traceFolder: string
): Promise<WorkerResult> {
  const snapshots: Snapshot[] = [];
  if (test.expectedStatus === "skipped") {
    reporter.startTest(test, {
      status: "pending",
      duration: 0,
      snapshots,
    });
    return {
      status: "skipped",
      duration: 0,
      snapshots,
    };
  }

  return new Promise((resolve) => {
    let startTime = Date.now();
    let reportStarted = false;
    let stdout = "";
    let stderr = "";
    try {
      const poolPromise = pool.exec(
        "testWorker",
        [
          test.id,
          getMockSuite(test),
          updateSnapshot,
          trace,
          importPath,
          attempt,
          traceFolder,
          shellReadyTimeout,
        ],
        {
          on: (payload) => {
            if (payload.stdout) {
              stdout += payload.stdout;
            }
            if (payload.stderr) {
              stderr += payload.stderr;
            }
            if (payload.errorMessage) {
              resolve({
                status: "unexpected",
                error: payload.errorMessage,
                duration: payload.duration,
                snapshots,
                stdout,
                stderr,
              });
            } else if (payload.startTime && !reportStarted) {
              reporter.startTest(test, {
                status: "pending",
                duration: 0,
                snapshots,
              });
              reportStarted = true;
              startTime = payload.startTime;
            } else if (payload.snapshotResult) {
              snapshots.push({
                name: payload.snapshotName,
                result: payload.snapshotResult,
              });
            }
          },
        }
      );
      if (timeout > 0) {
        poolPromise.timeout(timeout);
      }
      poolPromise
        .then(() => {
          if (!reportStarted) {
            reporter.startTest(test, {
              status: "pending",
              duration: 0,
              snapshots,
            });
          }
          resolve({
            status: "expected",
            duration: Date.now() - startTime,
            snapshots,
            stdout,
            stderr,
          });
        })
        .catch((e: unknown) => {
          const duration = startTime != null ? Date.now() - startTime : -1;
          if (!reportStarted) {
            reporter.startTest(test, {
              status: "pending",
              duration: 0,
              snapshots,
            });
          }
          if (typeof e === "string") {
            resolve({
              status: "unexpected",
              error: e,
              duration,
              snapshots,
              stdout,
              stderr,
            });
          } else if (e instanceof workerpool.Promise.TimeoutError) {
            resolve({
              status: "unexpected",
              error: `Error: worker was terminated as the timeout (${timeout} ms) was exceeded`,
              duration,
              snapshots,
              stdout,
              stderr,
            });
          } else if (e instanceof Error) {
            resolve({
              status: "unexpected",
              error: e.stack ?? e.message,
              duration,
              snapshots,
              stdout,
              stderr,
            });
          } else {
            resolve({
              status: "unexpected",
              error: `Unknown worker error: ${String(e)}`,
              duration,
              snapshots,
              stdout,
              stderr,
            });
          }
        });
    } catch (e) {
      const duration = startTime != null ? Date.now() - startTime : -1;
      if (!reportStarted) {
        reporter.startTest(test, {
          status: "pending",
          duration: 0,
          snapshots,
        });
      }
      if (typeof e === "string") {
        resolve({
          status: "unexpected",
          error: e,
          duration,
          snapshots,
          stdout,
          stderr,
        });
      } else if (e instanceof workerpool.Promise.TimeoutError) {
        resolve({
          status: "unexpected",
          error: `Error: worker was terminated as the timeout (${timeout} ms) was exceeded`,
          duration,
          snapshots,
          stdout,
          stderr,
        });
      } else if (e instanceof Error) {
        resolve({
          status: "unexpected",
          error: e.stack ?? e.message,
          duration,
          snapshots,
          stdout,
          stderr,
        });
      }
    }
  });
}

const getMockSuite = (test: TestCase): Suite => {
  let testSuite: Suite | undefined = test.suite;
  const newSuites: Suite[] = [];
  while (testSuite != null) {
    if (testSuite.type !== "describe") {
      newSuites.push(
        new Suite(testSuite.title, testSuite.type, testSuite.options)
      );
    }
    testSuite = testSuite.parentSuite;
  }
  for (let i = 0; i < newSuites.length - 1; i++) {
    newSuites[i].parentSuite = newSuites[i + 1];
  }
  return newSuites[0];
};

const testWorker = async (
  testId: string,
  testSuite: Suite,
  updateSnapshot: boolean,
  trace: boolean,
  importPath: string,
  attempt: number,
  traceFolder: string,
  shellReadyTimeout: number
): Promise<void> => {
  flushSnapshotExecutionCache();

  const startTime = Date.now();
  const tracePoints = [{ data: "", time: startTime }];
  workerpool.workerEmit({
    startTime,
  });
  try {
    await runTest(
      testId,
      testSuite,
      updateSnapshot,
      trace,
      tracePoints,
      importPath,
      shellReadyTimeout
    );
  } catch (e) {
    let errorMessage;
    if (typeof e == "string") {
      errorMessage = e;
    } else if (e instanceof Error) {
      errorMessage = e.stack ?? e.message;
    }
    if (errorMessage) {
      const duration = Date.now() - startTime;
      workerpool.workerEmit({
        errorMessage,
        duration,
      });
    }
  }
  if (trace) {
    await saveTrace(tracePoints, testId, attempt, traceFolder);
  }
};

const afterAllWorker = async (): Promise<void> => {
  const hooks = [...activeSuites].reverse().flatMap((s) => s.afterAllHooks);
  for (const hook of hooks) {
    await Promise.resolve(hook());
  }
  activeSuites.length = 0;
};

if (!workerpool.isMainThread) {
  process.on("uncaughtException", () => {
    // prevent worker crashes from unhandled native errors
  });
  process.on("unhandledRejection", () => {
    // prevent worker crashes from unhandled promise rejections
  });
  workerpool.worker({
    testWorker: testWorker,
    afterAllWorker: afterAllWorker,
  });
}
