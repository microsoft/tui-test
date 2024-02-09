// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import type { MatcherContext, AsyncExpectationResult } from "expect";
import convert from "color-convert";
import chalk from "chalk";

import { getExpectTimeout } from "../../config/config.js";
import { Cell, Locator } from "../../terminal/locator.js";

export async function toHaveBgColor(
  this: MatcherContext,
  locator: Locator,
  expected: string | number | [number, number, number],
  options?: { timeout?: number }
): AsyncExpectationResult {
  const cells = await locator.resolve(options?.timeout ?? getExpectTimeout());
  const [result, errorCell] = hasBgColor(cells, expected, this.isNot ?? false);
  const pass = this.isNot ? !result : result;
  const badColor = toMatchingColorMode(expected, errorCell);

  return {
    pass,
    message: () => {
      if (!pass && !this.isNot) {
        return (
          `expect(${chalk.red("received")}).toHaveBgColor(${chalk.green("expected")})` +
          `\n\nExpected Color: ${chalk.green(expected.toString())}\nFound Color: ${chalk.red(badColor)} in cell "${errorCell?.termCell?.getChars()}" at ${errorCell?.x},${errorCell?.y}`
        );
      }
      if (pass && this.isNot) {
        return (
          `expect(${chalk.red("received")}).not.toHaveBgColor(${chalk.green("expected")})` +
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
    return termCell?.isBgDefault()
      ? "000000"
      : termCell?.isBgPalette()
        ? convert.ansi256.hex(termCell.getBgColor())
        : termCell?.getBgColor().toString(16) ?? "";
  } else if (Array.isArray(expected)) {
    return termCell?.isBgDefault()
      ? "[0,0,0]"
      : termCell?.isBgPalette()
        ? convert.ansi256.rgb(termCell.getBgColor()).toString()
        : convert.hex.rgb(termCell!.getBgColor().toString(16)).toString();
  } else {
    return termCell?.isBgDefault()
      ? "0"
      : termCell?.isBgPalette()
        ? termCell.getBgColor().toString()
        : convert.hex.ansi256(termCell!.getBgColor().toString(16)).toString();
  }
}

function hasBgColor(
  cells: Cell[],
  color: string | number | [number, number, number],
  isNot: boolean
): [boolean, Cell | undefined] {
  if (Array.isArray(color)) {
    const [red, green, blue] = color;
    const badCells = cells.filter((cell) => {
      const { termCell } = cell;
      const valid = termCell?.isBgDefault()
        ? red == 0 && blue == 0 && green == 0
        : termCell?.isBgPalette()
          ? termCell.getBgColor() == convert.rgb.ansi256(color)
          : termCell?.getBgColor().toString(16) === convert.rgb.hex(color);
      return isNot ? valid : !valid;
    });
    if (badCells.length > 0) return [false, badCells[0]];
  } else if (typeof color == "number") {
    const badCells = cells.filter((cell) => {
      const { termCell } = cell;
      const valid = termCell?.isBgDefault()
        ? color === -1 || color === 0
        : termCell?.isBgPalette()
          ? termCell.getBgColor() === color
          : termCell?.getBgColor().toString(16) === convert.ansi256.hex(color);
      return isNot ? valid : !valid;
    });
    if (badCells.length > 0) return [false, badCells[0]];
  } else if (typeof color == "string") {
    const badCells = cells.filter((cell) => {
      const { termCell } = cell;
      const valid = termCell?.isBgDefault()
        ? convert.hex.ansi256(color) === 0
        : termCell?.isBgPalette()
          ? termCell.getBgColor() === convert.hex.ansi256(color)
          : termCell?.getBgColor().toString(16) === color;
      return isNot ? valid : !valid;
    });
    if (badCells.length > 0) return [false, badCells[0]];
  }
  return [true, undefined];
}
