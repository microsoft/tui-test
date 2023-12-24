import { TestCase, TestResult } from "../test/testcase.js";
import { Shell } from "../terminal/shell.js";
import chalk from "chalk";
import { loadShellVersions } from "./utils.js";

export class BaseReporter {
  protected currentTest: number;

  constructor() {
    this.currentTest = 0;
  }

  async start(testCount: number, shells: Shell[]): Promise<void> {
    const shellVersions = await loadShellVersions(shells);
    process.stdout.write(`${chalk.gray("Running")} ${testCount} ${chalk.gray(`test${testCount > 1 ? "s" : ""} using the following shells:`)}\n`);
    shellVersions.forEach(({ shell, version, target }) => {
      process.stdout.write(shell + chalk.gray(" version ") + version + chalk.gray(" running from ") + target + "\n");
    });
    process.stdout.write("\n" + (shellVersions.length == 0 ? "\n" : ""));
  }
  startTest(test: TestCase, result: TestResult): void {
    this.currentTest += 1;
  }
  endTest(test: TestCase, result: TestResult): void {}
  end(): void {}
}
