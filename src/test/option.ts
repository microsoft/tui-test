// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { Shell } from "../terminal/shell.js";

export interface TestOptions {
  /**
   * The shell to initialize the terminal with. Defaults to `cmd` on Windows and `bash` on macOS/Linux.
   */
  shell?: Shell;

  /**
   * The number of rows to initialize the terminal with. Defaults to 30.
   */
  rows?: number;

  /**
   * The number of columns to initialize the terminal with. Defaults to 80.
   */
  columns?: number;

  /**
   * Environment to be set for the shell.
   */
  env?: { [key: string]: string | undefined };

  /**
   * The program to initialize the terminal with. Overrides the `shell` option
   */
  program?: {
    /**
     * The file to launch
     */
    file: string;

    /**
     * The file's arguments as argv
     */
    args?: string[];
  };
}
