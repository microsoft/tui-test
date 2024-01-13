#!/usr/bin/env node

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/* eslint-disable header/header */

import { run } from "../runner/runner.js";

type RootCommandOptions = {
  updateSnapshot: boolean | undefined;
};

export const action = async (options: RootCommandOptions) => {
  const { updateSnapshot } = options;
  await run({ updateSnapshot: updateSnapshot ?? false });
};
