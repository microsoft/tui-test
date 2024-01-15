// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { Command } from "commander";
import { run } from "../runner/runner.js";

type TactCommandOptions = {
  updateSnapshot: boolean | undefined;
};

const action = async (
  testFilter: string[] | undefined,
  options: TactCommandOptions
) => {
  const { updateSnapshot } = options;
  await run({ updateSnapshot: updateSnapshot ?? false, testFilter });
};

const cmd = new Command("test")
  .summary("run tests with tact")
  .description(
    `run tests with tact

Examples:
  \`npx @microsoft/tact test my.spec.ts\`
  \`npx @microsoft/tact test some.spec.ts:42\``
  )
  .argument(
    "[test-filter...]",
    "Pass an argument to filter test files. Each argument is treated as a regular expression. Matching is performed against the absolute file paths"
  )
  .option("-u, --updateSnapshot", `use this flag to re-record snapshots`)
  .action(action);

export default cmd;
