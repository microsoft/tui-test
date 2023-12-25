import { Shell } from "../terminal/shell.js";
import { fitToWidth, ansi } from "./utils.js";
import chalk from "chalk";
import { TestCase, TestResult } from "../test/testcase.js";
import { BaseReporter } from "./base.js";
import ms from "pretty-ms";

export class ListReporter extends BaseReporter {
  private _testRows: { [key: string]: number };

  constructor() {
    super();
    this._testRows = {};
  }

  override async start(testCount: number, shells: Shell[]): Promise<void> {
    await super.start(testCount, shells);
  }

  override startTest(test: TestCase, result: TestResult): void {
    super.startTest(test, result);
    if (!this.isTTY) {
      return;
    }
    this._testRows[test.id] = this.currentTest;
    const fullName = test.titlePath();

    const prefix = this._linePrefix(test, result);
    const line = chalk.dim(fullName.join(" › ")) + this._lineSuffix(test, result);
    this._appendLine(line, prefix);
  }

  override endTest(test: TestCase, result: TestResult): void {
    super.endTest(test, result);
    const fullName = test.titlePath();
    const prefix = this._linePrefix(test, result);
    const line = this._resultColor(result)(fullName.join(" › ")) + this._lineSuffix(test, result);
    const row = this._testRows[test.id];

    this._updateOrAppendLine(row, line, prefix);
  }

  override end(): void {
    super.end();
  }

  private _resultColor(result: TestResult): (str: string) => string {
    switch (result.status) {
      case "passed":
        return chalk.green;
      case "failed":
        return chalk.red;
      case "pending":
        return (str: string) => str;
    }
  }

  private _linePrefix(test: TestCase, result: TestResult): string {
    const row = this._testRows[test.id] ?? this.currentTest;
    const resultTag = result.status === "passed" ? chalk.green("✔") : result.status === "failed" ? chalk.red("✘") : " ";
    return `  ${resultTag}  ${chalk.dim(row)} `;
  }

  private _lineSuffix(test: TestCase, result: TestResult): string {
    const timeTag = chalk.dim(` (${ms(result.duration)})`);
    const retryIdx = test.results.length - (result.status === "pending" ? 0 : 1);
    const retryTag = retryIdx > 0 ? chalk.yellow(` (retry #${retryIdx})`) : "";
    return `${retryTag}${timeTag}`;
  }

  private _appendLine(line: string, prefix: string) {
    process.stdout.write(prefix + fitToWidth(line, process.stdout.columns, prefix) + "\n");
  }

  private _updateLine(row: number, line: string, prefix: string) {
    const offset = -(row - this.currentTest - 1);
    const updateAnsi = ansi.cursorPreviousLine.repeat(offset) + ansi.eraseCurrentLine;
    const restoreAnsi = ansi.cursorNextLine.repeat(offset);
    process.stdout.write(updateAnsi + prefix + fitToWidth(line, process.stdout.columns, prefix) + restoreAnsi);
  }

  private _updateOrAppendLine(row: number, line: string, prefix: string) {
    if (this.isTTY) {
      this._updateLine(row, line, prefix);
    } else {
      this._appendLine(line, prefix);
    }
  }
}
