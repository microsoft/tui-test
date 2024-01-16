// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import type { MatcherContext, AsyncExpectationResult } from "expect";
import { diffStringsUnified } from "jest-diff";
import path from "node:path";
import process from "node:process";
import fs from "node:fs";
import fsAsync from "node:fs/promises";
import workpool from "workerpool";

import { Terminal } from "../../terminal/term.js";

export type SnapshotStatus = "passed" | "failed" | "written" | "updated";

const snapshots = new Map<string, string>();
const snapshotsIdx = new Map<string, number>();
const snapshotPath = (testPath: string): string =>
  path.join(
    process.cwd(),
    path.dirname(testPath),
    "__snapshots__",
    `${path.basename(testPath)}.snap.cjs`
  );

const loadSnapshot = async (
  testPath: string,
  testName: string
): Promise<string | undefined> => {
  let snaps;
  if (snapshots.has(testPath)) {
    snaps = snapshots.get(testPath);
  } else {
    const snapPath = snapshotPath(testPath);
    if (!fs.existsSync(snapPath)) {
      return;
    }
    snaps = (await import(`file://${snapPath}`)).default;
    snapshots.set(testPath, snaps);
  }

  return Object.hasOwn(snaps, testName) ? snaps[testName].trim() : undefined;
};

const updateSnapshot = async (
  testPath: string,
  testName: string,
  snapshot: string
): Promise<void> => {
  const snapPath = snapshotPath(testPath);
  if (!fs.existsSync(snapPath)) {
    await fsAsync.mkdir(path.dirname(snapPath), { recursive: true });
  }

  const fh = await fsAsync.open(snapPath, "w+");
  const snapshots = (await import(`file://${snapPath}`)).default;
  snapshots[testName] = snapshot;

  await fh.writeFile(
    Object.keys(snapshots)
      .sort()
      .map(
        (snapshotName) =>
          `exports[\`${snapshotName}\`] = String.raw\`\n${snapshots[snapshotName].trim()}\n\`;\n\n`
      )
      .join("")
  );
  await fh.close();
};

const generateSnapshot = (terminal: Terminal, includeColors: boolean) => {
  const { view, shifts } = terminal.serialize();
  if (shifts.size === 0 || !includeColors) {
    return view;
  }
  return `${view}\n${JSON.stringify(Object.fromEntries(shifts), null, 2)}`;
};

export async function toMatchSnapshot(
  this: MatcherContext,
  terminal: Terminal,
  options?: { includeColors?: boolean }
): AsyncExpectationResult {
  const testName = (this.currentConcurrentTestName || (() => ""))() ?? "";
  const snapshotIdx = snapshotsIdx.get(testName) ?? 0;
  const snapshotPostfixTestName =
    snapshotIdx != null && snapshotIdx != 0
      ? `${testName} ${snapshotIdx}`
      : testName;
  snapshotsIdx.set(testName, snapshotIdx + 1);
  const existingSnapshot = await loadSnapshot(
    this.testPath ?? "",
    snapshotPostfixTestName
  );
  const newSnapshot = generateSnapshot(
    terminal,
    options?.includeColors ?? false
  );
  const snapshotsDifferent = existingSnapshot !== newSnapshot;
  const snapshotShouldUpdate =
    globalThis.__expectState.updateSnapshot && snapshotsDifferent;
  const snapshotEmpty = existingSnapshot == null;

  if (!workpool.isMainThread) {
    const snapshotResult = snapshotEmpty
      ? "written"
      : snapshotShouldUpdate
        ? "updated"
        : snapshotsDifferent
          ? "failed"
          : "passed";
    workpool.workerEmit({ snapshotResult, testName });
  }

  if (snapshotEmpty || snapshotShouldUpdate) {
    await updateSnapshot(
      this.testPath ?? "",
      snapshotPostfixTestName,
      newSnapshot
    );
    return Promise.resolve({
      pass: true,
      message: () => "",
    });
  }

  return Promise.resolve({
    pass: !snapshotsDifferent,
    message: !snapshotsDifferent
      ? () => ""
      : () => diffStringsUnified(existingSnapshot, newSnapshot ?? ""),
  });
}
