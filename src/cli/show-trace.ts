// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { Command } from "commander";

import { loadTrace } from "../trace/tracer.js";
import { play } from "../trace/viewer.js";

const action = async (traceFile: string) => {
  const trace = await loadTrace(traceFile);
  await play(trace);
};

const cmd = new Command("show-trace")
  .description(`view traces in the console`)
  .argument("<trace-file>", "the trace to replay in the terminal");

cmd.action(action);

export default cmd;
