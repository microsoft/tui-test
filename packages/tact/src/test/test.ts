import { Suite, Test, TestFunction, TestMap } from "../suite.js";
export { Shell } from "../terminal/shell.js";
import { TactTestOptions } from "./option.js";
import { expect as expectLib } from "expect";
import { toHaveValue } from "./matchers.js";
import { Matchers, AsymmetricMatchers, BaseExpect } from "expect";
import { Terminal } from "../terminal/term.js";
import { TactTestConfig } from "../config/config.js";

declare global {
  var suite: Suite;
  var updateWorkerCache: ((test: Test) => void) | undefined;
  var tests: TestMap | undefined;
}

/**
 * These tests are executed in tact environment that launches a shell and provides a fresh pty session to each test.
 * @param title Test title.
 * @param testFunction The test function that is run when calling the test function.
 */
export function test(title: string, testFunction: TestFunction) {
  const test = {
    title,
    id: suite.tests.length,
    testFunction,
    results: [],
  };
  if (globalThis.tests != null) {
    globalThis.tests[test.id] = test;
  }
  if (globalThis.updateWorkerCache) {
    globalThis.updateWorkerCache(test);
  }
  globalThis.suite.tests.push(test);
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
}

expectLib.extend({
  toHaveValue,
});

interface TerminalAssertions {
  /**
   * Checks that Terminal has the provided text or RegExp.
   *
   * **Usage**
   *
   * ```js
   * await expect(terminal).toHaveValue("> ");
   * ```
   *
   * @param options
   */
  toHaveValue(
    value: string | RegExp,
    options?: {
      /**
       * Time to retry the assertion for in milliseconds. Defaults to `timeout` in `TestConfig.expect`.
       */
      timeout?: number;
      /**
       * Whether to check the entire terminal buffer for the value instead of only the visible section.
       */
      full?: number;
    }
  ): Promise<void>;
}

declare type BaseMatchers<T> = Matchers<void, T> & Inverse<Matchers<void, T>> & PromiseMatchers<T>;
declare type AllowedGenericMatchers<T> = Pick<Matchers<void, T>, "toBe" | "toBeDefined" | "toBeFalsy" | "toBeNull" | "toBeTruthy" | "toBeUndefined">;
declare type SpecificMatchers<T> = T extends Terminal ? TerminalAssertions & AllowedGenericMatchers<T> : BaseMatchers<T>;

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

const expect = expectLib as Expect;

export { expect };

/**
 * Defines tact config
 */
export function defineConfig(config: TactTestConfig): TactTestConfig {
  return config;
}
