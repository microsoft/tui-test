// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import type { MatcherContext, AsyncExpectationResult } from "expect";
import convert from "color-convert";
import chalk from "chalk";

import { getExpectTimeout } from "../../config/config.js";
import { Cell, Locator } from "../../terminal/locator.js";

export async function toHaveFgColor(
  this: MatcherContext,
  locator: Locator,
  expected: string | number | [number, number, number],
  options?: { timeout?: number }
): AsyncExpectationResult {
  const cells = await locator.resolve(options?.timeout ?? getExpectTimeout());
  const [result, errorCell] = hasFgColor(
    cells ?? [],
    expected,
    this.isNot ?? false
  );
  const pass = this.isNot ? !result : result;
  const badColor = toMatchingColorMode(expected, errorCell);

  return {
    pass,
    message: () => {
      if (!pass && !this.isNot) {
        return (
          `expect(${chalk.red("received")}).toHaveFgColor(${chalk.green("expected")})` +
          `\n\nExpected Color: ${chalk.green(expected.toString())}\nFound Color: ${chalk.red(badColor)} in cell "${errorCell?.termCell?.getChars()}" at ${errorCell?.x},${errorCell?.y}`
        );
      }
      if (pass && this.isNot) {
        return (
          `expect(${chalk.red("received")}).not.toHaveFgColor(${chalk.green("expected")})` +
          `\n\nExpected No Occurrences Of Color: ${chalk.green(expected.toString())}\nFound Color: ${chalk.red(badColor)} in cell "${errorCell?.termCell?.getChars()}" at ${errorCell?.x},${errorCell?.y}`
        );
      }
      return "passed";
    },
  };
}

function toMatchingColorMode(
  expected: string | number | [number, number, number],
  cell?: Cell
): string {
  if (cell == null) return "";

  const { termCell } = cell;
  if (typeof expected == "string") {
    return termCell?.isFgDefault()
      ? "000000"
      : termCell?.isFgPalette()
        ? convert.ansi256.hex(termCell.getFgColor())
        : termCell?.getFgColor().toString(16) ?? "";
  } else if (Array.isArray(expected)) {
    return termCell?.isFgDefault()
      ? "[0,0,0]"
      : termCell?.isFgPalette()
        ? convert.ansi256.rgb(termCell.getFgColor()).toString()
        : convert.hex.rgb(termCell!.getFgColor().toString(16)).toString();
  } else {
    return termCell?.isFgDefault()
      ? "0"
      : termCell?.isFgPalette()
        ? termCell.getFgColor().toString()
        : convert.hex.ansi256(termCell!.getFgColor().toString(16)).toString();
  }
}

function hasFgColor(
  cells: Cell[],
  color: string | number | [number, number, number],
  isNot: boolean
): [boolean, Cell | undefined] {
  if (Array.isArray(color)) {
    const [red, green, blue] = color;
    const badCells = cells.filter((cell) => {
      const { termCell } = cell;
      const valid = termCell?.isFgDefault()
        ? red == 0 && blue == 0 && green == 0
        : termCell?.isFgPalette()
          ? termCell.getFgColor() == convert.rgb.ansi256(color)
          : termCell?.getFgColor().toString(16) === convert.rgb.hex(color);
      return isNot ? valid : !valid;
    });
    if (badCells.length > 0) return [false, badCells[0]];
  } else if (typeof color == "number") {
    const badCells = cells.filter((cell) => {
      const { termCell } = cell;
      const valid = termCell?.isFgDefault()
        ? color === -1 || color === 0
        : termCell?.isFgPalette()
          ? termCell.getFgColor() === color
          : termCell?.getFgColor().toString(16) === convert.ansi256.hex(color);
      return isNot ? valid : !valid;
    });
    if (badCells.length > 0) return [false, badCells[0]];
  } else if (typeof color == "string") {
    const badCells = cells.filter((cell) => {
      const { termCell } = cell;
      const valid = termCell?.isFgDefault()
        ? convert.hex.ansi256(color) === 0
        : termCell?.isFgPalette()
          ? termCell.getFgColor() === convert.hex.ansi256(color)
          : termCell?.getFgColor().toString(16) === color;
      return isNot ? valid : !valid;
    });
    if (badCells.length > 0) return [false, badCells[0]];
  }
  return [true, undefined];
}
