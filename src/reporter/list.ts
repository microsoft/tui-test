// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import ms from "pretty-ms";
import chalk from "chalk";

import { Shell } from "../terminal/shell.js";
import { fitToWidth, ansi } from "./utils.js";
import { TestCase, TestResult, TestStatus } from "../test/testcase.js";
import { BaseReporter, StaleSnapshotSummary } from "./base.js";
import { Suite } from "../test/suite.js";

export class ListReporter extends BaseReporter {
  private _testRows: { [key: string]: number };

  constructor() {
    super();
    this._testRows = {};
  }

  override async start(
    testCount: number,
    shells: Shell[],
    maxWorkers: number
  ): Promise<void> {
    await super.start(testCount, shells, maxWorkers);
  }

  override startTest(test: TestCase, result: TestResult): void {
    super.startTest(test, result);
    if (!this.isTTY) {
      return;
    }
    this._testRows[test.id] = this.currentTest;
    const fullName = test.titlePath();

    const prefix = this._linePrefix(test, "start");
    const line =
      chalk.dim(fullName.join(" › ")) + this._lineSuffix(test, result);
    this._appendLine(line, prefix);
  }

  override endTest(test: TestCase, result: TestResult): void {
    super.endTest(test, result);
    const fullName = test.titlePath();
    const prefix = this._linePrefix(test, "end");
    const line =
      this._resultColor(test.outcome())(fullName.join(" › ")) +
      this._lineSuffix(test, result);
    const row = this._testRows[test.id];

    this._updateOrAppendLine(row, line, prefix);
  }

  override end(
    rootSuite: Suite,
    staleSnapshotSummary: StaleSnapshotSummary
  ): number {
    return super.end(rootSuite, staleSnapshotSummary);
  }

  private _resultIcon(status: TestStatus): string {
    const color = this._resultColor(status);
    switch (status) {
      case "flaky":
      case "expected":
        return color("✔");
      case "unexpected":
        return color("✘");
      case "skipped":
      case "pending":
        return color("-");
      default:
        return " ";
    }
  }

  private _linePrefix(test: TestCase, testPoint: "start" | "end"): string {
    const row = this._testRows[test.id] ?? this.currentTest;
    return testPoint === "end"
      ? `  ${this._resultIcon(test.outcome())}  ${chalk.dim(row)} `
      : `  ${this._resultIcon("pending")}  ${chalk.dim(row)} `;
  }

  private _lineSuffix(test: TestCase, result: TestResult): string {
    const timeTag =
      result.status === "pending" ? "" : chalk.dim(` (${ms(result.duration)})`);
    const retryIdx =
      test.results.length - (result.status === "pending" ? 0 : 1);
    const retryTag = retryIdx > 0 ? chalk.yellow(` (retry #${retryIdx})`) : "";
    return `${retryTag}${timeTag}`;
  }

  private _appendLine(line: string, prefix: string) {
    process.stdout.write(
      prefix + fitToWidth(line, process.stdout.columns, prefix) + "\n"
    );
  }

  private _updateLine(row: number, line: string, prefix: string) {
    const offset = -(row - this.currentTest - 1);
    const updateAnsi =
      ansi.cursorPreviousLine.repeat(offset) + ansi.eraseCurrentLine;
    const restoreAnsi = ansi.cursorNextLine.repeat(offset);
    process.stdout.write(
      updateAnsi +
        prefix +
        fitToWidth(line, process.stdout.columns, prefix) +
        restoreAnsi
    );
  }

  private _updateOrAppendLine(row: number, line: string, prefix: string) {
    if (this.isTTY) {
      this._updateLine(row, line, prefix);
    } else {
      this._appendLine(line, prefix);
    }
  }
}
