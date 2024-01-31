// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { Command } from "commander";

import { getVersion } from "./version.js";
import { executableName } from "../utils/constants.js";
import { run } from "../runner/runner.js";

type CommandOptions = {
  updateSnapshot: boolean | undefined;
};

const action = async (
  testFilter: string[] | undefined,
  options: CommandOptions
) => {
  const { updateSnapshot } = options;
  await run({ updateSnapshot: updateSnapshot ?? false, testFilter });
};

export const program = new Command();

program
  .name(executableName)
  .description(
    `a fast and precise end-to-end terminal testing framework

Examples:
\`npx @microsoft/tui-test my.spec.ts\`
\`npx @microsoft/tui-test some.spec.ts:42\``
  )
  .argument(
    "[test-filter...]",
    "Pass an argument to filter test files. Each argument is treated as a regular expression. Matching is performed against the absolute file paths"
  )
  .option("-u, --updateSnapshot", `use this flag to re-record snapshots`)
  .version(await getVersion(), "-v, --version", "output the current version")
  .action(action)
  .showHelpAfterError("(add --help for additional information)");
