#!/usr/bin/env node

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/* eslint-disable header/header */

import { Command } from "commander";

import test from "./test.js";
import { getVersion } from "./version.js";

const program = new Command();

program
  .name("tact")
  .description("A fast and precise end-to-end terminal testing framework")
  .version(await getVersion(), "-v, --version", "output the current version")
  .action(() => program.help())
  .showHelpAfterError("(add --help for additional information)");

program.addCommand(test);

program.parse();
