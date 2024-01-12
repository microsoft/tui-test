// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/* eslint-disable header/header */

import { Command } from "commander";

import { action } from "./root.js";
import { getVersion } from "./version.js";

const program = new Command();

program
  .name("tact")
  .description("A fast and precise end-to-end terminal testing framework")
  .version(await getVersion(), "-v, --version", "output the current version")
  .action(action)
  .option("-u, --updateSnapshot", `use this flag to re-record snapshots`)
  .showHelpAfterError("(add --help for additional information)");

program.parse();
