import { TestCase, TestResult } from "../test/testcase.js";
import { Shell } from "../terminal/shell.js";
import chalk from "chalk";
import os from "node:os";
import { loadShellVersions } from "./utils.js";

const maxWorkers = Math.max(os.cpus().length - 1, 1);

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
  end(): void {}
}
