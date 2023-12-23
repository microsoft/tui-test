import { Suite, Test } from "../suite.js";
import { Shell } from "../terminal/shell.js";

export interface Reporter {
  start(testCount: number, shells: Shell[]): Promise<void>;
  startTest(test: Test, suite: Suite): void;
  endTest(test: Test, suite: Suite): void;
  end(): void;
}
