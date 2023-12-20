import type { MatcherContext, ExpectationResult, AsyncExpectationResult } from "expect";
import chalk from "chalk";

import { Terminal } from "../core/term.js";

export function toHaveValue(this: MatcherContext, terminal: Terminal, expected: string | RegExp): ExpectationResult {
  const viewableBuffer = terminal
    .getViewableBuffer()
    .map((bufferLine) => bufferLine.join(""))
    .join("");
  const pass = typeof expected === "string" ? viewableBuffer.includes(expected) : expected.test(viewableBuffer);
  return {
    pass,
    message: () => {
      if (!pass) {
        const comparisonMethod = typeof expected === "string" ? "String.prototype.includes search" : "RegExp.prototype.test search";
        return (
          `Error: expect(${chalk.red("received")}).toHaveValue(${chalk.green("expected")}) ${chalk.gray("// " + comparisonMethod)}` +
          `\n\nExpected: ${chalk.green(expected.toString())}\nMatches Found: ${chalk.red(0)}`
        );
      }
      return "passed";
    },
  };
}

// TODO: implement matcher
export function toHaveValueVisible(
  this: MatcherContext,
  terminal: Terminal,
  expected: string | RegExp,
  options?: { timeout?: number }
): AsyncExpectationResult {
  return Promise.resolve({
    pass: true,
    message: () => "",
  });
}

// TODO: implement matcher
export function toMatchSnapshot(this: MatcherContext, terminal: Terminal): AsyncExpectationResult {
  return Promise.resolve({
    pass: true,
    message: () => "",
  });
}
