// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import path from "node:path";
import url from "node:url";
import os from "node:os";
import workerpool, { Pool } from "workerpool";
import chalk from "chalk";

import { Suite, getRootSuite } from "../test/suite.js";
import { transformFiles } from "./transform.js";
import {
  TestConfig,
  getRetries,
  getShellReadyTimeout,
  getTimeout,
  loadConfig,
} from "../config/config.js";
import { runTestWorker } from "./worker.js";
import { Shell, setupZshDotfiles } from "../terminal/shell.js";
import { ListReporter } from "../reporter/list.js";
import { BaseReporter } from "../reporter/base.js";
import { TestCase } from "../test/testcase.js";
import { cacheFolderName, executableName } from "../utils/constants.js";
import { supportsColor } from "chalk";
import { cleanSnapshot } from "../test/matchers/toMatchSnapshot.js";
import { isBun } from "../utils/runtime.js";

/* eslint-disable no-var */

declare global {
  var suite: Suite;
  var tests: { [testId: string]: TestCase };
}

type ExecutionOptions = {
  updateSnapshot: boolean;
  trace?: boolean;
  testFilter?: string[];
};

const __dirname = path.dirname(url.fileURLToPath(import.meta.url));

const createWorkerPool = (maxWorkers: number): Pool => {
  return workerpool.pool(path.join(__dirname, "worker.js"), {
    workerType: "process",
    maxWorkers,
    forkOpts: {
      stdio: "inherit",
      env: {
        ...process.env,
        ...(supportsColor ? { FORCE_COLOR: "1" } : {}),
      },
    },
    emitStdStreams: true,
  });
};

const runSuites = async (
  allSuites: Suite[],
  filteredTestIds: Set<string>,
  reporter: BaseReporter,
  options: ExecutionOptions,
  config: Required<TestConfig>,
  maxWorkers: number
) => {
  const { updateSnapshot } = options;
  const trace = options.trace ?? config.trace;

  const groupTestsByFile = (suites: Suite[]): TestCase[][] => {
    const fileTests = new Map<string, TestCase[]>();
    const visit = (suites: Suite[]) => {
      for (const suite of suites) {
        for (const test of suite.tests) {
          if (!filteredTestIds.has(test.id)) continue;
          const file = test.filePath() ?? "";
          if (!fileTests.has(file)) fileTests.set(file, []);
          fileTests.get(file)!.push(test);
        }
        visit(suite.suites);
      }
    };
    visit(suites);
    return Array.from(fileTests.values());
  };

  const files = groupTestsByFile(allSuites);
  const runNextFile = async (): Promise<void> => {
    while (files.length != 0) {
      const tests = files.shift();
      if (!tests) break;
      const filePool = createWorkerPool(1);
      try {
        for (const test of tests) {
          for (let i = 0; i < Math.max(0, getRetries()) + 1; i++) {
            const testResult = await runTestWorker(
              test,
              test.sourcePath()!,
              {
                timeout: getTimeout(),
                updateSnapshot,
                shellReadyTimeout: getShellReadyTimeout(),
              },
              trace,
              filePool,
              reporter,
              i,
              config.traceFolder
            );
            test.results.push(testResult);
            reporter.endTest(test, testResult);
            if (
              testResult.status == "skipped" ||
              testResult.status == test.expectedStatus
            )
              break;
          }
        }
        await filePool.exec("afterAllWorker", []);
      } finally {
        try {
          await filePool.terminate(true);
        } catch {
          /* empty */
        }
      }
    }
  };

  await Promise.all(
    Array.from({ length: Math.min(maxWorkers, files.length) }, () =>
      runNextFile()
    )
  );
};

const checkRuntimeVersion = () => {
  if (isBun()) {
    if (os.platform() === "win32") {
      console.error(
        chalk.red(
          `Error: ${executableName} does not support Bun on Windows. Please use Node.js instead.\n`
        )
      );
      process.exit(1);
    }
    if (Bun.version.localeCompare("1.3.5", undefined, { numeric: true }) < 0) {
      console.warn(
        chalk.yellow(
          `Warning: ${executableName} works best with Bun version 1.3.5 or higher (you are using ${Bun.version}).\n`
        )
      );
    }
    return;
  }
  const nodeVersion = process.versions.node;
  const nodeMajorVersion = nodeVersion.split(".")[0].trim();
  if (
    nodeMajorVersion != "16" &&
    nodeMajorVersion != "18" &&
    nodeMajorVersion != "20" &&
    nodeMajorVersion != "22" &&
    nodeMajorVersion != "24"
  ) {
    console.warn(
      chalk.yellow(
        `Warning: ${executableName} works best when using a supported node versions (which ${nodeVersion} is not).\n`
      )
    );
  }
};

const checkShellSupport = (shells: Shell[]) => {
  let platform = "";
  let badShells: string[] = [];
  if (os.platform() === "darwin") {
    badShells = shells.filter(
      (shell) =>
        shell == Shell.Cmd ||
        shell == Shell.Powershell ||
        shell == Shell.WindowsPowershell
    );
    platform = "macOS";
  } else if (os.platform() == "win32") {
    badShells = shells.filter(
      (shell) => shell == Shell.Zsh || shell == Shell.Fish
    );
    platform = "Windows";
  } else if (os.platform() == "linux") {
    badShells = shells.filter(
      (shell) => shell == Shell.Cmd || shell == Shell.WindowsPowershell
    );
    platform = "Linux";
  }
  if (badShells.length != 0) {
    console.warn(
      chalk.yellow(
        `Warning: ${executableName} does not support the following shells on ${platform} ${badShells.join(", ")}`
      )
    );
  }
};

const cleanSnapshots = async (
  tests: TestCase[],
  { updateSnapshot }: ExecutionOptions
) => {
  const snapshotFiles = tests.reduce((snapshots, test) => {
    const source = test.filePath() ?? "";
    const snapshotNames = test.snapshots().map((snapshot) => snapshot.name);
    snapshots.set(source, [...(snapshots.get(source) ?? []), ...snapshotNames]);
    return snapshots;
  }, new Map<string, string[]>());

  let unusedSnapshots = 0;
  for (const snapshotFile of snapshotFiles.keys()) {
    unusedSnapshots += await cleanSnapshot(
      snapshotFile,
      new Set(snapshotFiles.get(snapshotFile)),
      updateSnapshot
    );
  }
  return unusedSnapshots;
};

export const run = async (options: ExecutionOptions) => {
  checkRuntimeVersion();

  await transformFiles();
  const config = await loadConfig();
  const rootSuite = await getRootSuite(config);
  const reporter = new ListReporter();

  const suites = [rootSuite];
  while (suites.length != 0) {
    const importSuite = suites.shift();
    if (importSuite?.type === "file") {
      const transformedSuitePath = path.join(
        process.cwd(),
        cacheFolderName,
        importSuite.title
      );
      const parsedSuitePath = path.parse(transformedSuitePath);
      const extension = parsedSuitePath.ext.startsWith(".m") ? ".mjs" : ".js";
      const importablePath = `file://${path.join(parsedSuitePath.dir, `${parsedSuitePath.name}${extension}`)}`;
      importSuite.source = importablePath;
      globalThis.suite = importSuite;
      await import(importablePath);
    }
    suites.push(...(importSuite?.suites ?? []));
  }

  let allTests = rootSuite.allTests();

  // refine with only annotations
  if (allTests.find((test) => test.annotations.includes("only"))) {
    allTests = allTests.filter((test) => test.annotations.includes("only"));
  }

  // refine with test filters
  if (options.testFilter != null && options.testFilter.length > 0) {
    try {
      const patterns = options.testFilter.map(
        (filter) => new RegExp(filter.replaceAll("\\", "\\\\"))
      );
      allTests = allTests.filter((test) => {
        const testPath = path.resolve(test.filePath() ?? "");
        return patterns.find((pattern) => pattern.test(testPath)) != null;
      });
    } catch {
      console.error(
        "Error: invalid test filter supplied. Test filters must be valid regular expressions"
      );
      process.exit(1);
    }
  }

  const shells = Array.from(
    new Set(
      allTests
        .map((t) => t.suite.options?.shell)
        .filter((s): s is Shell => s != null)
    )
  );
  checkShellSupport(shells);
  if (shells.includes(Shell.Zsh)) {
    await setupZshDotfiles();
  }
  await reporter.start(allTests.length, shells, config.workers);

  if (config.globalTimeout > 0) {
    setTimeout(() => {
      console.error(
        `Error: global timeout (${config.globalTimeout} ms) exceeded`
      );
      process.exit(1);
    }, config.globalTimeout);
  }

  await runSuites(
    rootSuite.suites,
    new Set(allTests.map((test) => test.id)),
    reporter,
    options,
    config,
    config.workers
  );

  const staleSnapshots = await cleanSnapshots(allTests, options);
  const failures = reporter.end(rootSuite, {
    obsolete: options.updateSnapshot ? 0 : staleSnapshots,
    removed: options.updateSnapshot ? staleSnapshots : 0,
  });

  process.exit(failures);
};
