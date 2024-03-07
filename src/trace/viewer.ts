// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import readlineAsync from "node:readline/promises";
import { EventEmitter } from "node:events";
import chalk from "chalk";

import { DataTracePoint, SizeTracePoint, Trace } from "./tracer.js";
import ansi from "../terminal/ansi.js";

const rl = readlineAsync.createInterface({
  input: process.stdin,
  output: process.stdout,
});

export const play = async (trace: Trace) => {
  const startTime = trace.tracePoints.find(
    (tracePoint): tracePoint is DataTracePoint => "time" in tracePoint
  )!.time;
  const startSize = trace.tracePoints.find(
    (tracePoint): tracePoint is SizeTracePoint => "rows" in tracePoint
  )!;

  if (
    process.stdout.columns != startSize.cols ||
    process.stdout.rows != startSize.rows
  ) {
    console.warn(
      chalk.yellow(
        `Warning: the current terminal size (rows: ${process.stdout.rows}, columns: ${process.stdout.columns}) doesn't match the starting dimensions used in the trace (rows: ${startSize.rows}, columns: ${startSize.cols}).`
      )
    );
  }

  if (
    trace.tracePoints.filter((tracePoint) => "rows" in tracePoint).length > 1
  ) {
    console.warn(
      chalk.yellow(
        `Warning: the trace contains resize actions which won't be emulated when viewing.`
      )
    );
  }

  const answer = (await rl.question("\nDo you want to start the trace [y/N]? "))
    .trim()
    .toLowerCase();

  if (answer !== "y" && answer !== "yes") {
    process.stdout.write("Exiting trace viewer\n");
    process.exit(0);
  }

  const dataPoints = trace.tracePoints
    .filter(
      (tracePoint): tracePoint is DataTracePoint =>
        "time" in tracePoint && tracePoint.data != ""
    )
    .map((dataPoint) => ({ ...dataPoint, delay: dataPoint.time - startTime }));

  const totalEvents = dataPoints.length;
  let executedEvents = 0;
  const e = new EventEmitter();

  process.stdout.write(ansi.saveScreen);
  process.stdout.write(ansi.clearScreen);
  process.stdout.write(ansi.cursorTo(0, 0));

  dataPoints.forEach((dataPoint) => {
    setTimeout(() => {
      process.stdout.write(dataPoint.data);
      e.emit("write");
    }, dataPoint.delay);
  });

  e.on("write", async () => {
    executedEvents += 1;
    if (executedEvents == totalEvents) {
      await rl.question("\n\nReplay complete, press any key to exit ");
      process.stdout.write(
        ansi.cursorTo(process.stdout.rows, process.stdout.columns)
      );
      process.stdout.write(ansi.restoreScreen);
      process.stdout.write("\n\n");
      process.exit(0);
    }
  });

  return new Promise<void>(() => {});
};
