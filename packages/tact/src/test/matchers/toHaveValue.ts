// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import type { MatcherContext, AsyncExpectationResult } from "expect";
import chalk from "chalk";

import { Terminal } from "../../terminal/term.js";
import { poll } from "../utils.js";
import { getExpectTimeout } from "../../config/config.js";

export async function toHaveValue(
  this: MatcherContext,
  terminal: Terminal,
  expected: string | RegExp,
  options?: { timeout?: number; full?: boolean },
): AsyncExpectationResult {
  const pass = await poll(
    () => {
      const buffer = (
        options?.full === true
          ? terminal.getBuffer()
          : terminal.getViewableBuffer()
      )
        .map((bufferLine) => bufferLine.join(""))
        .join("");
      const pass =
        typeof expected === "string"
          ? buffer.includes(expected)
          : expected.test(buffer);
      return pass;
    },
    50,
    options?.timeout ?? getExpectTimeout(),
  );

  return {
    pass,
    message: () => {
      if (!pass) {
        const comparisonMethod =
          typeof expected === "string"
            ? "String.prototype.includes search"
            : "RegExp.prototype.test search";
        return (
          `expect(${chalk.red("received")}).toHaveValue(${chalk.green("expected")}) ${chalk.dim("// " + comparisonMethod)}` +
          `\n\nExpected: ${chalk.green(expected.toString())}\nMatches Found: ${chalk.red(0)}`
        );
      }
      return "passed";
    },
  };
}
