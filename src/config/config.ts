// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import { defaultShell } from "../terminal/shell.js";
import { TestOptions } from "../test/option.js";
import { cacheFolderName, configFileName } from "../utils/constants.js";

const configPath = path.join(process.cwd(), cacheFolderName, configFileName);
let loadedConfig: Required<TestConfig> | undefined;

export const loadConfig = async (): Promise<Required<TestConfig>> => {
  const userConfig: TestConfig = !fs.existsSync(configPath)
    ? {}
    : (await import(`file://${configPath}`)).default ?? {};
  loadedConfig = {
    testMatch: userConfig.testMatch ?? "**/*.@(spec|test).?(c|m)[jt]s?(x)",
    expect: {
      timeout: userConfig.timeout ?? 5_000,
    },
    globalTimeout: userConfig.globalTimeout ?? 0,
    retries: userConfig.retries ?? 0,
    projects: userConfig.projects ?? [],
    timeout: userConfig.timeout ?? 30_000,
    reporter: userConfig.reporter ?? "list",
    use: {
      shell: userConfig.use?.shell ?? defaultShell,
      rows: userConfig.use?.rows ?? 30,
      columns: userConfig.use?.columns ?? 80,
      program: userConfig.use?.program,
    },
  };
  return loadedConfig;
};

export const getExpectTimeout = (): number =>
  loadedConfig?.expect.timeout ?? 5_000;
export const getTimeout = (): number => loadedConfig?.timeout ?? 30_000;
export const getRetries = (): number => loadedConfig?.retries ?? 0;

declare type TestProject = {
  /**
   * Project name is visible in the report and during test execution.
   */
  name?: string;
};

declare type WorkerOptions = {
  /**
   * Only the files matching one of these patterns are executed as test files. Matching is performed against the
   * absolute file path. Strings are treated as glob patterns.
   *
   * By default, TUI Test looks for files matching the following glob pattern: `**\/*.@(spec|test).?(c|m)[jt]s?(x)`.
   * This means JavaScript or TypeScript files with `".test"` or `".spec"` suffix, for example
   * `bash.integration.spec.ts`.
   *
   * Use testConfig.testMatch to change this option for all projects.
   */
  testMatch?: string;
};

export declare type ProjectConfig = TestOptions &
  Required<WorkerOptions> &
  TestProject;

export declare type TestConfig = {
  /**
   * Configuration for the `expect` assertion library.
   *
   * **Usage**
   *
   * ```js
   * // tui-test.config.ts
   * import { defineConfig } from '@microsoft/tui-test';
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
   * // tui-test.config.ts
   * import { defineConfig } from '@microsoft/tui-test';
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
   * By default, TUI Test looks for files matching the following glob pattern: `**\/*.@(spec|test).?(c|m)[jt]s?(x)`.
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
   * // tui-test.config.ts
   * import { defineConfig } from '@microsoft/tui-test';
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
   * // tui-test.config.ts
   * import { defineConfig } from '@microsoft/tui-test';
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
   * // tui-test.config.ts
   * import { defineConfig } from '@microsoft/tui-test';
   *
   * export default defineConfig({
   *   reporter: 'list',
   * });
   * ```
   *
   */
  reporter?: "list";

  /**
   * Options for all tests in this project
   *
   * ```js
   * // tui-test.config.ts
   * import { defineConfig, Shell } from '@microsoft/tui-test';
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
   * TUI Test supports running multiple test projects at the same time.
   *
   * **Usage**
   *
   * ```js
   * // tui-test.config.ts
   * import { defineConfig, Shell } from '@microsoft/tui-test';
   *
   * export default defineConfig({
   *   projects: [
   *     { name: 'bash', use: Shell.Bash }
   *   ]
   * });
   * ```
   *
   */
  projects?: ProjectConfig[];
};
