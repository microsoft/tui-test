// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import {
  expect as jestExpect,
  Matchers,
  AsymmetricMatchers,
  BaseExpect,
} from "expect";
import path from "node:path";

import { Suite, suiteFilePath } from "./suite.js";
import { TestFunction, TestCase, Location } from "./testcase.js";
export { Shell } from "../terminal/shell.js";
import { TestOptions } from "./option.js";
import { toMatchSnapshot } from "./matchers/toMatchSnapshot.js";
import { Terminal } from "../terminal/term.js";
import { TestConfig } from "../config/config.js";
import { toHaveBgColor } from "./matchers/toHaveBgColor.js";
import { toHaveFgColor } from "./matchers/toHaveFgColor.js";
import { Locator } from "../terminal/locator.js";
import { toBeVisible } from "./matchers/toBeVisible.js";

/* eslint-disable no-var */

declare global {
  var suite: Suite;
  var tests: { [testId: string]: TestCase };
  var __expectState: { updateSnapshot: boolean };
}

const getTestLocation = () => {
  const filename = suiteFilePath(globalThis.suite)!;
  const errorStack = new Error().stack;
  let location: Location = { row: 0, column: 0 };
  if (errorStack) {
    const lineInfo = errorStack
      .match(new RegExp(`${path.basename(filename)}(.*)\\)`))
      ?.at(1)
      ?.split(":")
      ?.slice(-2);
    if (
      lineInfo?.length === 2 &&
      lineInfo.every((info) => /^\d+$/.test(info))
    ) {
      const [row, column] = lineInfo.map((info) => Number(info));
      location = { row, column };
    }
  }
  return location;
};

/**
 * These tests are executed in new terminal context which provides a shell and provides a new pty to each test.
 * @param title Test title.
 * @param testFunction The test function that is run when calling the test function.
 */
export function test(title: string, testFunction: TestFunction) {
  const location = getTestLocation();
  const test = new TestCase(title, location, testFunction, globalThis.suite);
  if (globalThis.tests != null) {
    globalThis.tests[test.id] = test;
  }
  globalThis.suite.tests.push(test);
}

// eslint-disable-next-line @typescript-eslint/no-namespace
export namespace test {
  /**
   * Specifies options or fixtures to use in a single test file or a test.describe group. Most useful to
   * set an option, for example set `shell` to configure the shell initialized for each test.
   *
   * **Usage**
   *
   * ```js
   * import { test, expect, Shell } from '@microsoft/tui-test';
   *
   * test.use({ shell: Shell.Cmd });
   *
   * test('test on cmd', async ({ terminal }) => {
   *   // The terminal now is running the shell that has been specified
   * });
   * ```
   *
   * **Details**
   *
   * `test.use` can be called either in the global scope or inside `test.describe`. It is an error to call it within
   * `beforeEach` or `beforeAll`.
   * ```
   *
   * @param options An object with local options.
   */
  export const use = (options: TestOptions) => {
    globalThis.suite.options = { ...globalThis.suite.options, ...options };
  };

  /**
   * Declares a group of tests.
   *
   * **Usage**
   *
   * ```js
   * test.describe('two tests', () => {
   *   test('one', async ({ terminal }) => {
   *     // ...
   *   });
   *
   *   test('two', async ({ terminal }) => {
   *     // ...
   *   });
   * });
   * ```
   *
   * @param title Group title.
   * @param callback A callback that is run immediately when calling test.describe
   * Any tests added in this callback will belong to the group.
   */
  export const describe = (title: string, callback: () => void) => {
    const parentSuite = globalThis.suite;
    const currentSuite = new Suite(
      title,
      "describe",
      parentSuite.options,
      parentSuite
    );
    parentSuite.suites.push(currentSuite);
    globalThis.suite = currentSuite;
    callback();
    globalThis.suite = parentSuite;
  };

  /**
   * Declares a skipped test. Skipped test is never run.
   *
   * **Usage**
   *
   * ```js
   * import { test, expect } from '@microsoft/tui-test';
   *
   * test.skip('broken test', async ({ page }) => {
   *   // ...
   * });
   * ```
   *
   * @param title Test title.
   * @param testFunction The test function that is run when calling the test function.
   */
  export const skip = (title: string, testFunction: TestFunction) => {
    const location = getTestLocation();
    const test = new TestCase(
      title,
      location,
      testFunction,
      globalThis.suite,
      "skipped"
    );
    if (globalThis.tests != null) {
      globalThis.tests[test.id] = test;
    }
    globalThis.suite.tests.push(test);
  };

  /**
   * Declares a failed test.
   *
   * **Usage**
   *
   * ```js
   * import { test, expect } from '@microsoft/tui-test';
   *
   * test.fail('purposely failing test', async ({ page }) => {
   *   // ...
   * });
   * ```
   *
   * @param title Test title.
   * @param testFunction The test function that is run when calling the test function.
   */
  export const fail = (title: string, testFunction: TestFunction) => {
    const location = getTestLocation();
    const test = new TestCase(
      title,
      location,
      testFunction,
      globalThis.suite,
      "unexpected"
    );
    if (globalThis.tests != null) {
      globalThis.tests[test.id] = test;
    }
    globalThis.suite.tests.push(test);
  };

  /**
   * Declares a conditional test.
   *
   * **Usage**
   *
   * ```js
   * import { test, expect } from '@microsoft/tui-test';
   * import os from "node:os";
   *
   * const isWindows = os.platform() == "win32"
   * test.when(isWindows, 'windows only test', async ({ page }) => {
   *   // ...
   * });
   * ```
   *
   * @param shouldRun If the test should be run or skipped.
   * @param title Test title.
   * @param testFunction The test function that is run when calling the test function.
   */
  export const when = (
    shouldRun: boolean,
    title: string,
    testFunction: TestFunction
  ) => {
    if (shouldRun) {
      test(title, testFunction);
    } else {
      skip(title, testFunction);
    }
  };

  /**
   * Declares a focused test. If there are some focused tests or suites, all of them will be run but nothing else.
   *
   * **Usage**
   *
   * ```js
   * import { test, expect } from '@microsoft/tui-test';
   *
   * test.only('focus this test', async ({ page }) => {
   *   // Run only focused tests
   * });
   * ```
   *
   * @param title Test title.
   * @param testFunction The test function that is run when calling the test function.
   */
  export const only = (title: string, testFunction: TestFunction) => {
    const location = getTestLocation();
    const test = new TestCase(
      title,
      location,
      testFunction,
      globalThis.suite,
      "expected",
      ["only"]
    );
    if (globalThis.tests != null) {
      globalThis.tests[test.id] = test;
    }
    globalThis.suite.tests.push(test);
  };
}

jestExpect.extend({
  toBeVisible,
  toMatchSnapshot,
  toHaveBgColor,
  toHaveFgColor,
});

interface TerminalMatchers {
  toMatchSnapshot(options?: {
    /**
     * Include color information in the snapshot.
     */
    includeColors?: boolean;
  }): Promise<void>;
}

interface LocatorMatchers {
  /**
   * Checks that selected text is visible.
   *
   * **Usage**
   *
   * ```js
   * await expect(terminal.getByText(">")).toBeVisible();
   * ```
   *
   * @param options
   */
  toBeVisible(options?: {
    /**
     * Time to retry the assertion for in milliseconds. Defaults to `timeout` in `TestConfig.expect`.
     */
    timeout?: number;
  }): Promise<void>;

  /**
   * Checks that selected text has the desired background color.
   *
   * **Usage**
   *
   * ```js
   * await expect(terminal.getByText(">")).toHaveBgColor("#000000");
   * ```
   *
   * @param value The desired cell's background color. Can be in the following forms
   * - ANSI 256: This is a number from 0 to 255 of ANSI colors `255`
   * - Hex: A string representing a 'true color' `#FFFFFF`
   * - RGB: An array presenting an rgb color `[255, 255, 255]`
   * @param options
   */
  toHaveBgColor(
    value: string | number | [number, number, number],
    options?: {
      /**
       * Time to retry the assertion for in milliseconds. Defaults to `timeout` in `TestConfig.expect`.
       */
      timeout?: number;
    }
  ): Promise<void>;

  /**
   * Checks that selected text has the desired foreground color.
   *
   * **Usage**
   *
   * ```js
   * await expect(terminal.getByText(">")).toHaveFgColor("#000000");
   * ```
   *
   * @param value The desired cell's foreground color. Can be in the following forms
   * - ANSI 256: This is a number from 0 to 255 of ANSI colors `255`
   * - Hex: A string representing a 'true color' `#FFFFFF`
   * - RGB: An array presenting an rgb color `[255, 255, 255]`
   * @param options
   */
  toHaveFgColor(
    value: string | number | [number, number, number],
    options?: {
      /**
       * Time to retry the assertion for in milliseconds. Defaults to `timeout` in `TestConfig.expect`.
       */
      timeout?: number;
    }
  ): Promise<void>;
}

declare type BaseMatchers<T> = Matchers<void, T> &
  Inverse<Matchers<void, T>> &
  PromiseMatchers<T>;

declare type AllowedGenericMatchers<T> = Pick<
  Matchers<void, T>,
  | "toBe"
  | "toBeDefined"
  | "toBeFalsy"
  | "toBeNull"
  | "toBeTruthy"
  | "toBeUndefined"
>;

declare type SpecificMatchers<T> = T extends Terminal
  ? TerminalMatchers &
      AllowedGenericMatchers<T> &
      Inverse<AllowedGenericMatchers<T>>
  : T extends Locator
    ? LocatorMatchers & Inverse<LocatorMatchers>
    : BaseMatchers<T>;

export declare type Expect = {
  <T = unknown>(actual: T): SpecificMatchers<T>;
} & BaseExpect &
  AsymmetricMatchers &
  Inverse<Omit<AsymmetricMatchers, "any" | "anything">>;

declare type PromiseMatchers<T = unknown> = {
  /**
   * Unwraps the reason of a rejected promise so any other matcher can be chained.
   * If the promise is fulfilled the assertion fails.
   */
  rejects: Matchers<Promise<void>, T> & Inverse<Matchers<Promise<void>, T>>;
  /**
   * Unwraps the value of a fulfilled promise so any other matcher can be chained.
   * If the promise is rejected the assertion fails.
   */
  resolves: Matchers<Promise<void>, T> & Inverse<Matchers<Promise<void>, T>>;
};

declare type Inverse<Matchers> = {
  /**
   * Inverse next matcher. If you know how to test something, `.not` lets you test its opposite.
   */
  not: Matchers;
};

const expect = jestExpect as Expect;

export { expect };

/**
 * Defines tui-test config
 */
export function defineConfig(config: TestConfig): TestConfig {
  return config;
}
