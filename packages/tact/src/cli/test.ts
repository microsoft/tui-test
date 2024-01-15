// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { Command } from "commander";
import { run } from "../runner/runner.js";

type TactCommandOptions = {
  updateSnapshot: boolean | undefined;
};

const action = async (options: TactCommandOptions) => {
  const { updateSnapshot } = options;
  await run({ updateSnapshot: updateSnapshot ?? false });
};

const cmd = new Command("test")
  .description(`run tests with tact`)
  .option("-u, --updateSnapshot", `use this flag to re-record snapshots`)
  .action(action);

export default cmd;
