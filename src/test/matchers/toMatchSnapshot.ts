// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import type { MatcherContext, AsyncExpectationResult } from "expect";
import { diffStringsUnified } from "jest-diff";
import path from "node:path";
import process from "node:process";
import fs from "node:fs";
import fsAsync from "node:fs/promises";
import module from "node:module";
import workpool from "workerpool";
import lockfile from "proper-lockfile";

import { Terminal } from "../../terminal/term.js";

export type SnapshotStatus = "passed" | "failed" | "written" | "updated";

const require = module.createRequire(import.meta.url);

const snapshots = new Map<string, string>();
const snapshotsIdx = new Map<string, number>();
const snapshotPath = (testPath: string): string =>
  path.join(
    process.cwd(),
    path.dirname(testPath),
    "__snapshots__",
    `${path.basename(testPath)}.snap`
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
    snaps = require(snapPath);
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
  if (!fs.existsSync(path.dirname(snapPath))) {
    await fsAsync.mkdir(path.dirname(snapPath), { recursive: true });
  }
  if (!fs.existsSync(snapPath)) {
    await fsAsync.appendFile(snapPath, "");
  }

  const unlock = await lockfile.lock(snapPath, {
    stale: 5_000,
    retries: {
      retries: 5,
      minTimeout: 50,
      maxTimeout: 1_000,
      randomize: true,
    },
  });

  delete require.cache[require.resolve(snapPath)];
  const snapshots = require(snapPath);
  snapshots[testName] = snapshot;
  await fsAsync.writeFile(
    snapPath,
    "// TUI Test Snapshot v1\n\n" +
      Object.keys(snapshots)
        .sort()
        .map(
          (snapshotName) =>
            `exports[\`${snapshotName}\`] = String.raw\`\n${snapshots[snapshotName].trim()}\n\`;\n\n`
        )
        .join("")
  );
  await unlock();
};

export const cleanSnapshot = async (
  testPath: string,
  retainedSnapshots: Set<string>,
  updateSnapshot: boolean
): Promise<number> => {
  const snapPath = snapshotPath(testPath);
  if (!fs.existsSync(snapPath)) return retainedSnapshots.size;
  const snapshots = require(snapPath);
  const unusedSnapshots = Object.keys(snapshots).filter(
    (snapshot) => !retainedSnapshots.has(snapshot)
  );

  if (!updateSnapshot) return unusedSnapshots.length;
  unusedSnapshots.forEach((unusedSnapshot) => {
    delete snapshots[unusedSnapshot];
  });

  await fsAsync.writeFile(
    snapPath,
    "// TUI Test Snapshot v1\n\n" +
      Object.keys(snapshots)
        .sort()
        .map(
          (snapshotName) =>
            `exports[\`${snapshotName}\`] = String.raw\`\n${snapshots[snapshotName].trim()}\n\`;\n\n`
        )
        .join("")
  );

  return unusedSnapshots.length;
};

const generateSnapshot = (terminal: Terminal, includeColors: boolean) => {
  const { view, shifts } = terminal.serialize();
  if (shifts.size === 0 || !includeColors) {
    return view;
  }
  return `${view}\n${JSON.stringify(Object.fromEntries(shifts), null, 2)}`;
};

export const flushSnapshotExecutionCache = () => snapshotsIdx.clear();

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

  const emitResult = () => {
    if (!workpool.isMainThread) {
      const snapshotResult: SnapshotStatus = snapshotEmpty
        ? "written"
        : snapshotShouldUpdate
          ? "updated"
          : snapshotsDifferent
            ? "failed"
            : "passed";
      workpool.workerEmit({
        snapshotResult,
        snapshotName: snapshotPostfixTestName,
      });
    }
  };

  if (snapshotEmpty || snapshotShouldUpdate) {
    await updateSnapshot(
      this.testPath ?? "",
      snapshotPostfixTestName,
      newSnapshot
    );
    emitResult();
    return {
      pass: true,
      message: () => "",
    };
  } else {
    emitResult();
  }

  return {
    pass: !snapshotsDifferent,
    message: !snapshotsDifferent
      ? () => ""
      : () => diffStringsUnified(existingSnapshot, newSnapshot ?? ""),
  };
}
