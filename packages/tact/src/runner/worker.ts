// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import process from "node:process";
import workerpool from "workerpool";

import { Suite } from "../test/suite.js";
import { spawn } from "../terminal/term.js";
import { defaultShell } from "../terminal/shell.js";
import { TestCase, TestStatus } from "../test/testcase.js";
import { expect } from "../test/test.js";
import { SnapshotStatus } from "../test/matchers/toMatchSnapshot.js";
import { BaseReporter } from "../reporter/base.js";

type WorkerResult = {
  error?: string;
  status: TestStatus;
  duration: number;
  snapshots: SnapshotStatus[];
};

type WorkerExecutionOptions = {
  timeout: number;
  updateSnapshot: boolean;
};

const importSet = new Set<string>();

const runTest = async (
  testId: string,
  testSuite: Suite,
  updateSnapshot: boolean,
  importPath: string
) => {
  process.setSourceMapsEnabled(true);
  globalThis.suite = testSuite;
  globalThis.tests = globalThis.tests ?? {};
  globalThis.__expectState = { updateSnapshot };
  if (!importSet.has(importPath)) {
    await import(importPath);
    importSet.add(importPath);
  }
  const test = globalThis.tests[testId];
  const { shell, rows, columns, env } = test.suite.options ?? {};
  const terminal = await spawn({
    shell: shell ?? defaultShell,
    rows: rows ?? 30,
    cols: columns ?? 80,
    env,
  });

  const allTests = Object.values(globalThis.tests);
  const testPath = test.filePath();
  const signatureIdenticalTests = allTests.filter(
    (t) => t.filePath() === testPath && t.title === test.title
  );
  const signatureIdx = signatureIdenticalTests.findIndex(
    (t) => t.id == test.id
  );
  const currentConcurrentTestName = () => `${test.title} ${signatureIdx + 1}`;

  expect.setState({
    ...expect.getState(),
    testPath,
    currentTestName: test.title,
    currentConcurrentTestName,
  });
  await Promise.resolve(test.testFunction({ terminal }));
  terminal.kill();
};

export async function runTestWorker(
  test: TestCase,
  importPath: string,
  { timeout, updateSnapshot }: WorkerExecutionOptions,
  pool: workerpool.Pool,
  reporter: BaseReporter
): Promise<WorkerResult> {
  const snapshots: SnapshotStatus[] = [];
  if (test.expectedStatus === "skipped") {
    return {
      status: "skipped",
      duration: 0,
      snapshots,
    };
  }

  return new Promise((resolve) => {
    let startTime = Date.now();
    let reportStarted = false;
    try {
      const poolPromise = pool.exec(
        "testWorker",
        [test.id, getMockSuite(test), updateSnapshot, importPath],
        {
          on: (payload) => {
            if (payload.errorMessage) {
              resolve({
                status: "unexpected",
                error: payload.errorMessage,
                duration: payload.duration,
                snapshots,
              });
            } else if (payload.startTime) {
              reporter.startTest(test, {
                status: "pending",
                duration: 0,
                snapshots,
              });
              reportStarted = true;
              startTime = payload.startTime;
            } else if (payload.snapshotResult) {
              snapshots.push(payload.snapshotResult);
            }
          },
        }
      );
      if (timeout > 0) {
        poolPromise.timeout(timeout);
      }
      poolPromise.then(() => {
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
        });
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
        });
      } else if (e instanceof workerpool.Promise.TimeoutError) {
        resolve({
          status: "unexpected",
          error: `Error: worker was terminated as the timeout (${timeout} ms) as exceeded`,
          duration,
          snapshots,
        });
      } else if (e instanceof Error) {
        resolve({
          status: "unexpected",
          error: e.stack ?? e.message,
          duration,
          snapshots,
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
  importPath: string
): Promise<void> => {
  const startTime = Date.now();
  workerpool.workerEmit({
    startTime,
  });
  try {
    await runTest(testId, testSuite, updateSnapshot, importPath);
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
};

if (!workerpool.isMainThread) {
  workerpool.worker({
    testWorker: testWorker,
  });
}
