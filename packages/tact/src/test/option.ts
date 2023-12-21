import { Shell } from "../terminal/shell.js";

export interface TactTestOptions {
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
}
