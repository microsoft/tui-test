// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import path from "node:path";
import fs from "node:fs";
import zlib from "node:zlib";
import process from "node:process";
import fsAsync from "node:fs/promises";
import { promisify } from "node:util";

export type TracePoint = DataTracePoint | SizeTracePoint;

export type DataTracePoint = {
  time: number;
  data: string;
};
export type SizeTracePoint = {
  rows: number;
  cols: number;
};

export type Trace = {
  tracePoints: TracePoint[];
  testPath: string[];
  testName: string[];
  attempt: number;
};

const zipInflate = promisify(zlib.inflate);
const zipDeflate = promisify(zlib.deflate);

const traceFilename = (testId: string, attempt: number) => {
  const test = globalThis.tests[testId];
  const filename = path
    .relative(process.cwd(), path.resolve(test.filePath()!))
    .replace(path.sep, "-");
  const title = test.titlePath().slice(1).join("-");
  const retry = attempt > 0 ? `-retry${attempt}` : "";
  const name = `${filename}-${title}${retry}`.replaceAll(
    /[ /\\<>:"'|?*]/g,
    "-"
  );
  return name;
};

const testName = (testId: string) => {
  const test = globalThis.tests[testId];
  const t = test.titlePath();
  const [filename, row, column] = t[0].split(":");
  const testPath = [
    ...path.relative(process.cwd(), path.resolve(filename)).split(path.sep),
    row,
    column,
  ];
  const testName = t.slice(1);
  return { testName, testPath };
};

export const loadTrace = async (traceFilename: string): Promise<Trace> => {
  if (!fs.existsSync(traceFilename)) {
    throw new Error("unable to load trace, file not found");
  }
  return JSON.parse(
    (await zipInflate(await fsAsync.readFile(traceFilename))).toString()
  );
};

export const saveTrace = async (
  tracePoints: TracePoint[],
  testId: string,
  attempt: number,
  traceFolder: string
) => {
  const filename = traceFilename(testId, attempt);
  if (!fs.existsSync(traceFolder)) {
    await fsAsync.mkdir(traceFolder, { recursive: true });
  }
  const trace: Trace = {
    tracePoints,
    attempt,
    ...testName(testId),
  };

  await fsAsync.writeFile(
    path.join(traceFolder, filename),
    await zipDeflate(Buffer.from(JSON.stringify(trace), "utf8"))
  );
};
