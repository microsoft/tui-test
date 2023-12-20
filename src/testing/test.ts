import { Suite, TestFunction } from "../core/suite.js";
export { Shell } from "../core/shell.js";
import { TactTestOptions } from "./option.js";
import { expect as expectLib } from "expect";
import { toHaveValue } from "./matchers.js";
import type { Expect } from "./types.js";

declare global {
  var suite: Suite;
}

/**
 * These tests are executed in tact environment that launches a shell and provides a fresh pty session to each test.
 * @param title Test title.
 * @param testFunction The test function that is run when calling the test function.
 */
export function test(title: string, testFunction: TestFunction) {
  suite.tests.push({
    title,
    testFunction,
  });
}

export namespace test {
  /**
   * Specifies options or fixtures to use in a single test file or a test.describe group. Most useful to
   * set an option, for example set `shell` to configure the shell initialized for each test.
   *
   * **Usage**
   *
   * ```js
   * import { test, expect, Shell } from '@microsoft/tact-test';
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
  export const use = (options: TactTestOptions) => {
    globalThis.suite.use(options);
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
    const currentSuite = new Suite(title, "describe", parentSuite.options(), parentSuite);
    parentSuite.suites.push(currentSuite);
    globalThis.suite = currentSuite;
    callback();
    globalThis.suite = parentSuite;
  };

  /**
   * Waits the specified number of ms until continuing
   *
   * **Usage**
   *
   * ```js
   * test.wait(1_000); // waits for 1 second
   * test.wait(1_500) // waits for 1,500 milliseconds
   * ```
   *
   * @param time amount of time to wait in ms
   */
  export const wait = (time: number) => new Promise((resolve) => setTimeout(resolve, time));
}

expectLib.extend({
  toHaveValue,
});

const expect = expectLib as Expect;

export { expect };
