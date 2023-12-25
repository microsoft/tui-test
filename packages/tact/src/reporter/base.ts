import os from "node:os";
import chalk from "chalk";

import { TestCase, TestResult, TestStatus } from "../test/testcase.js";
import { Shell } from "../terminal/shell.js";
import { loadShellVersions } from "./utils.js";
import { Suite } from "../test/suite.js";

const maxWorkers = Math.max(os.cpus().length - 1, 1);

type TestSummary = {
  didNotRun: number;
  skipped: number;
  expected: number;
  unexpected: TestCase[];
  flaky: TestCase[];
  failuresToPrint: TestCase[];
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
          workers
        )} with the following shells:\n`
      )
    );

    shellVersions.forEach(({ shell, version, target }) => {
      process.stdout.write(shell + chalk.dim(" version ") + version + chalk.dim(" running from ") + target + "\n");
    });
    process.stdout.write("\n" + (shellVersions.length == 0 ? "\n" : ""));
  }
  startTest(test: TestCase, result: TestResult): void {
    if (this.isTTY) this.currentTest += 1;
  }
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

    rootSuite.allTests().forEach((test) => {
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
    };
  }

  private _printSummary({ didNotRun, skipped, expected, unexpected, flaky }: TestSummary) {
    const tokens = [];
    if (unexpected.length) {
      tokens.push(chalk.red(`  ${unexpected.length} failed`));
      for (const test of unexpected) tokens.push(chalk.red(this._header(test)));
    }
    if (flaky.length) {
      tokens.push(chalk.yellow(`  ${flaky.length} flaky`));
      for (const test of flaky) tokens.push(chalk.yellow(this._header(test)));
    }
    if (didNotRun > 0) {
      tokens.push(chalk.yellow(`  ${didNotRun} did not run`));
    }
    if (skipped > 0) {
      tokens.push(chalk.yellow(`  ${didNotRun} skipped`));
    }
    if (expected > 0) {
      tokens.push(chalk.green(`  ${expected} passed`));
    }
    process.stdout.write(tokens.join("\n") + "\n");
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
          resultIdx === 0 ? this._resultColor(test.outcome())(this._header(test, `  ${failureIdx + 1}) `)) : chalk.dim(this._retryHeader(resultIdx));
        process.stdout.write("\n" + header + "\n\n" + padError(result.error) + "\n\n");
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
