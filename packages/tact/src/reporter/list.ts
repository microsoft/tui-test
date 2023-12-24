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
    this._testRows[test.id] = this.currentTest;
    const fullName = test.titlePath();
    const retryTag = test.results.length > 0 ? ` ${chalk.yellow(`(retry #${test.results.length})`)}` : "";
    const prefix = `     ${chalk.gray(this.currentTest)} `;
    const line = `${chalk.gray(fullName.join(" › "))}${retryTag}`;
    process.stdout.write(prefix + fitToWidth(line, process.stdout.columns, prefix) + "\n");
  }

  override endTest(test: TestCase, result: TestResult): void {
    const fullName = test.titlePath();
    const retryTag = test.results.length > 1 ? ` ${chalk.yellow(`(retry #${test.results.length - 1})`)}` : "";
    const timeTag = ` ${chalk.gray(`(${ms(result.duration)})`)}`;
    const row = this._testRows[test.id];
    const resultTag = result.passed ? chalk.green("✔") : chalk.red("✘");
    const prefix = `  ${resultTag}  ${chalk.gray(row)} `;
    const resultColor = result.passed ? chalk.green : chalk.red;
    const line = `${resultColor(fullName.join(" › "))}${retryTag}${timeTag}`;

    const offset = -(row - this.currentTest - 1);
    process.stdout.write(
      ansi.cursorPreviousLine.repeat(offset) +
        ansi.eraseCurrentLine +
        prefix +
        fitToWidth(line, process.stdout.columns, prefix) +
        ansi.cursorNextLine.repeat(offset)
    );

    super.endTest(test, result);
  }

  override end(): void {
    super.end();
  }
}
