import { Matchers, AsymmetricMatchers, BaseExpect } from "expect";
import { Terminal } from "../core/term.ts";
import { TactTestOptions as TestOptions } from "./option.ts";

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

declare type WorkerOptions = {
  /**
   * Only the files matching one of these patterns are executed as test files. Matching is performed against the
   * absolute file path. Strings are treated as glob patterns.
   *
   * By default, Tact looks for files matching the following glob pattern: `**\/*.@(spec|test).?(c|m)[jt]s?(x)`.
   * This means JavaScript or TypeScript files with `".test"` or `".spec"` suffix, for example
   * `bash.integration.spec.ts`.
   *
   * Use testConfig.testMatch to change this option for all projects.
   */
  testMatch?: string;
};

declare type TestProject = {
  /**
   * Project name is visible in the report and during test execution.
   */
  name?: string;
};

export declare type TactProjectConfig = TestOptions & Required<WorkerOptions> & TestProject;

export declare type TactTestConfig = {
  /**
   * Configuration for the `expect` assertion library.
   *
   * **Usage**
   *
   * ```js
   * // tact.config.ts
   * import { defineConfig } from '@microsoft/tact-test';
   *
   * export default defineConfig({
   *   expect: {
   *     timeout: 10000,
   *   },
   * });
   * ```
   *
   */
  expect?: {
    /**
     * Default timeout for async expect matchers in milliseconds, defaults to 5000ms.
     */
    timeout?: number;
  };
  /**
   * Timeout for each test in milliseconds. Defaults to 30 seconds.
   *
   * This is a base timeout for all tests.
   *
   * **Usage**
   *
   * ```js
   * // tact.config.ts
   * import { defineConfig } from '@microsoft/tact-test';
   *
   * export default defineConfig({
   *   timeout: 5 * 60 * 1000,
   * });
   * ```
   *
   */
  timeout?: number;

  /**
   * Only the files matching one of these patterns are executed as test files. Matching is performed against the
   * absolute file path. Strings are treated as glob patterns.
   *
   * By default, Tact looks for files matching the following glob pattern: `**\/*.@(spec|test).?(c|m)[jt]s?(x)`.
   * This means JavaScript or TypeScript files with `".test"` or `".spec"` suffix, for example
   * `bash.integration.spec.ts`.
   *
   * Use testConfig.testMatch to change this option for all projects.
   */
  testMatch?: string;

  /**
   * Maximum time in milliseconds the whole test suite can run. Zero timeout (default) disables this behavior. Useful on
   * CI to prevent broken setup from running too long and wasting resources.
   *
   * **Usage**
   *
   * ```js
   * // tact.config.ts
   * import { defineConfig } from '@microsoft/tact-test';
   *
   * export default defineConfig({
   *   globalTimeout: process.env.CI ? 60 * 60 * 1000 : undefined,
   * });
   * ```
   *
   */
  globalTimeout?: number;

  /**
   * The maximum number of retry attempts given to failed tests. By default failing tests are not retried.
   *
   * **Usage**
   *
   * ```js
   * // tact.config.ts
   * import { defineConfig } from '@microsoft/tact-test';
   *
   * export default defineConfig({
   *   retries: 2,
   * });
   * ```
   *
   */
  retries?: number;

  /**
   * The list of builtin reporters to use.
   *
   * **Usage**
   *
   * ```js
   * // playwright.config.ts
   * import { defineConfig } from '@playwright/test';
   *
   * export default defineConfig({
   *   reporter: 'list',
   * });
   * ```
   *
   */
  reporter?: LiteralUnion<"list" | "null", string>;

  /**
   * Options for all tests in this project
   *
   * ```js
   * // tact.config.ts
   * import { defineConfig, Shell } from '@microsoft/tact-test';
   *
   * export default defineConfig({
   *   projects: [
   *     {
   *       name: 'bash',
   *       use: {
   *         shell: Shell.Bash,
   *       },
   *     },
   *   ],
   * });
   * ```
   *
   * Use testConfig.use to change this option for
   * all projects.
   */
  use?: TestOptions;

  /**
   * Tact supports running multiple test projects at the same time.
   *
   * **Usage**
   *
   * ```js
   * // playwright.config.ts
   * import { defineConfig, Shell } from '@microsoft/tact-test';
   *
   * export default defineConfig({
   *   projects: [
   *     { name: 'bash', use: Shell.Bash }
   *   ]
   * });
   * ```
   *
   */
  projects?: Array<TactProjectConfig>;
};
