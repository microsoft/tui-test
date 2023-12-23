import { Shell } from "../terminal/shell.js";
import { getFullName, fitToWidth, ansi } from "./utils.js";
import { Reporter } from "./reporter.js";
import chalk from "chalk";
import { Suite, Test } from "../suite.js";
import { BaseReporter } from "./base.js";
import ms from "pretty-ms";

export class ListReporter extends BaseReporter implements Reporter {
  private _testRows: { [key: string]: number };

  constructor() {
    super();
    this._testRows = {};
  }

  override async start(testCount: number, shells: Shell[]): Promise<void> {
    await super.start(testCount, shells);
  }

  override startTest(test: Test, suite: Suite): void {
    super.startTest(test, suite);
    this._testRows[`${test.globalId}${test.results.length}`] = this.currentTest;

    const fullName = getFullName(test, suite);
    const retryTag = test.results.length > 0 ? ` ${chalk.yellow(`(retry #${test.results.length})`)}` : "";
    const prefix = `     ${chalk.gray(this.currentTest)} `;
    const line = `${chalk.gray(fullName.join(" › "))}${retryTag}`;
    process.stdout.write(prefix + fitToWidth(line, process.stdout.columns, prefix) + "\n");
  }

  override endTest(test: Test, suite: Suite): void {
    const result = test.results.at(-1)!;
    const fullName = getFullName(test, suite);
    const retryTag = test.results.length > 1 ? ` ${chalk.yellow(`(retry #${test.results.length - 1})`)}` : "";
    const timeTag = ` ${chalk.gray(`(${ms(result.executionTime)})`)}`;
    const row = this.getRow(test);
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

    super.endTest(test, suite);
  }

  override end(): void {
    super.end();
  }

  private getRow(test: Test): number {
    return this._testRows[`${test.globalId}${test.results.length - 1}`];
  }
}
