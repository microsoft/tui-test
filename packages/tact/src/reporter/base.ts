// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import chalk from "chalk";

import { TestCase, TestResult, TestStatus } from "../test/testcase.js";
import { Shell } from "../terminal/shell.js";
import { loadShellVersions } from "./utils.js";
import { Suite } from "../test/suite.js";
import { maxWorkers } from "../runner/runner.js";

type TestSummary = {
  didNotRun: number;
  skipped: number;
  expected: number;
  unexpected: TestCase[];
  flaky: TestCase[];
  failuresToPrint: TestCase[];
  snapshots: {
    passed: number;
    failed: number;
    written: number;
    updated: number;
  };
};

export class BaseReporter {
  protected currentTest: number;
  protected isTTY: boolean;

  constructor() {
    this.currentTest = 0;
    this.isTTY = process.stdout.isTTY;
  }

  private _plural(text: string, count: number): string {
    return `${text}${count > 1 ? "s" : ""}`;
  }

  async start(testCount: number, shells: Shell[]): Promise<void> {
    const shellVersions = await loadShellVersions(shells);
    const workers = Math.min(testCount, maxWorkers);
    process.stdout.write(
      chalk.dim(
        `Running ${chalk.dim.reset(testCount)} ${this._plural("test", testCount)} using ${chalk.dim.reset(workers)} ${this._plural(
          "worker",
          workers,
        )} with the following shells:\n`,
      ),
    );

    shellVersions.forEach(({ shell, version, target }) => {
      process.stdout.write(
        shell +
          chalk.dim(" version ") +
          version +
          chalk.dim(" running from ") +
          target +
          "\n",
      );
    });
    process.stdout.write("\n" + (shellVersions.length == 0 ? "\n" : ""));
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  startTest(test: TestCase, result: TestResult): void {
    if (this.isTTY) this.currentTest += 1;
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  endTest(test: TestCase, result: TestResult): void {
    if (!this.isTTY) this.currentTest += 1;
  }
  end(rootSuite: Suite): number {
    const summary = this._generateSummary(rootSuite);

    this._printFailures(summary);
    this._printSummary(summary);
    return summary.failuresToPrint.length;
  }

  private _generateSummary(rootSuite: Suite): TestSummary {
    let didNotRun = 0;
    let skipped = 0;
    let expected = 0;
    const unexpected: TestCase[] = [];
    const flaky: TestCase[] = [];
    const snapshots = { written: 0, updated: 0, failed: 0, passed: 0 };

    rootSuite.allTests().forEach((test) => {
      test.snapshots().forEach((snapshot) => {
        switch (snapshot) {
          case "passed":
            snapshots.passed++;
            break;
          case "failed":
            snapshots.failed++;
            break;
          case "written":
            snapshots.written++;
            break;
          case "updated":
            snapshots.updated++;
            break;
        }
      });
      switch (test.outcome()) {
        case "skipped": {
          if (!test.results.length) {
            ++didNotRun;
          } else {
            ++skipped;
          }
          break;
        }
        case "expected":
          ++expected;
          break;
        case "unexpected":
          unexpected.push(test);
          break;
        case "flaky":
          flaky.push(test);
          break;
      }
    });

    const failuresToPrint = [...unexpected, ...flaky];
    return {
      didNotRun,
      skipped,
      expected,
      unexpected,
      flaky,
      failuresToPrint,
      snapshots,
    };
  }

  private _printSummary({
    didNotRun,
    skipped,
    expected,
    unexpected,
    flaky,
    snapshots,
  }: TestSummary) {
    const tokens = [];
    if (unexpected.length) {
      tokens.push(chalk.red(`${unexpected.length} failed`));
    }
    if (flaky.length) {
      tokens.push(chalk.yellow(`${flaky.length} flaky`));
      for (const test of flaky) tokens.push(chalk.yellow(this._header(test)));
    }
    if (didNotRun > 0) {
      tokens.push(chalk.yellow(`${didNotRun} did not run`));
    }
    if (skipped > 0) {
      tokens.push(chalk.yellow(`${skipped} skipped`));
    }
    if (expected > 0) {
      tokens.push(chalk.green(`${expected} passed`));
    }

    const testTotal =
      unexpected.length + flaky.length + didNotRun + skipped + expected;
    if (testTotal !== 0) {
      process.stdout.write(
        `\n  tests: ${tokens.join(", ")}, ${testTotal} total\n`,
      );
    }

    const snapshotTokens = [];
    if (snapshots.passed > 0) {
      snapshotTokens.push(chalk.green(`${snapshots.passed} passed`));
    }
    if (snapshots.failed > 0) {
      snapshotTokens.push(chalk.red(`${snapshots.failed} failed`));
    }
    if (snapshots.updated > 0) {
      snapshotTokens.push(chalk.green(`${snapshots.updated} updated`));
    }
    if (snapshots.written > 0) {
      snapshotTokens.push(chalk.green(`${snapshots.written} written`));
    }

    const snapshotTotal =
      snapshots.passed +
      snapshots.failed +
      snapshots.written +
      snapshots.updated;
    const snapshotErrorPostfix =
      snapshots.failed > 0
        ? chalk.dim(
            "(Inspect your code changes or use the `-u` flag to update them.)",
          )
        : "";
    if (snapshotTotal !== 0) {
      process.stdout.write(
        `  snapshots: ${snapshotTokens.join(", ")}, ${snapshotTotal} total ${snapshotErrorPostfix}\n\n`,
      );
    } else {
      process.stdout.write("\n");
    }
  }

  private _header(test: TestCase, prefix?: string): string {
    const line = (prefix ?? "     ") + test.titlePath().join(" › ");
    const stdoutWidth = process.stdout.columns - 4;
    const padLength = stdoutWidth > 96 ? 96 : stdoutWidth;
    return line.padEnd(padLength, "─");
  }

  private _retryHeader(retry: number) {
    const stdoutWidth = process.stdout.columns - 4;
    const padLength = stdoutWidth > 96 ? 96 : stdoutWidth;
    return `     Retry #${retry} `.padEnd(padLength, "─");
  }

  private _printFailures({ failuresToPrint }: TestSummary) {
    const padError = (error: string) =>
      error
        .split("\n")
        .map((line) => `     ${line}`)
        .join("\n");
    failuresToPrint.forEach((test, failureIdx) => {
      test.results.forEach((result, resultIdx) => {
        if (result.error == null) return;
        const header =
          resultIdx === 0
            ? this._resultColor(test.outcome())(
                this._header(test, `  ${failureIdx + 1}) `),
              )
            : chalk.dim(this._retryHeader(resultIdx));
        process.stdout.write(
          "\n" + header + "\n\n" + padError(result.error) + "\n\n",
        );
      });
    });
  }

  protected _resultColor(status: TestStatus): (str: string) => string {
    switch (status) {
      case "expected":
        return chalk.green;
      case "unexpected":
        return chalk.red;
      case "flaky":
        return chalk.yellow;
      case "skipped":
        return chalk.cyan;
      default:
        return (str: string) => str;
    }
  }
}
