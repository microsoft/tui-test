#!/usr/bin/env node

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/* eslint-disable header/header */

export * from "./lib/test/test.js";
import { program } from "./lib/cli/index.js";
import { isMain } from "./lib/utils/main.js";

if (isMain(import.meta)) {
  program.parse();
}
