import { Test, Suite } from "../suite.js";
import { Shell } from "../terminal/shell.js";
import { Reporter } from "./reporter.js";
import chalk from "chalk";
import { loadShellVersions } from "./utils.js";

export class BaseReporter implements Reporter {
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
  startTest(test: Test, suite: Suite): void {
    this.currentTest += 1;
  }
  endTest(test: Test, suite: Suite): void {}
  end(): void {}
}
