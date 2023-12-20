import type { MatcherContext, ExpectationResult, AsyncExpectationResult } from "expect";
import chalk from "chalk";

import { Terminal } from "../core/term.js";
import { poll } from "./utils.js";

export async function toHaveValue(
  this: MatcherContext,
  terminal: Terminal,
  expected: string | RegExp,
  options?: { timeout?: number; full?: boolean }
): AsyncExpectationResult {
  const pass = await poll(
    () => {
      const buffer = (options?.full === true ? terminal.getBuffer() : terminal.getViewableBuffer()).map((bufferLine) => bufferLine.join("")).join("");
      const pass = typeof expected === "string" ? buffer.includes(expected) : expected.test(buffer);
      return pass;
    },
    50,
    10_000 // TODO: once configuration is supported, change this value
  );

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
export function toMatchSnapshot(this: MatcherContext, terminal: Terminal): AsyncExpectationResult {
  return Promise.resolve({
    pass: true,
    message: () => "",
  });
}
