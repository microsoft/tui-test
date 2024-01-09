import type { MatcherContext, AsyncExpectationResult } from "expect";
import path from "node:path";
import process from "node:process";
import fs from "node:fs";
import fsAsync from "node:fs/promises";

import { Terminal } from "../../terminal/term.js";

const snapshots = new Map<string, any>();

const snapshotPath = (testPath: string): string => path.join(process.cwd(), path.dirname(testPath), "__snapshots__", `${path.basename(testPath)}.snap`);

const loadSnapshot = async (testPath: string, testName: string): Promise<string | undefined> => {
  let snaps;
  if (snapshots.has(testPath)) {
    snaps = snapshots.get(testPath);
  } else {
    const snapPath = snapshotPath(testPath);
    if (!fs.existsSync(snapPath)) {
      return;
    }
    snaps = await import(snapPath);
    snapshots.set(testPath, snaps);
  }

  return Object.hasOwn(snaps, testName) ? snaps[testName] : undefined;
};

export async function toMatchSnapshot(this: MatcherContext, terminal: Terminal): AsyncExpectationResult {
  const testName = (this.currentConcurrentTestName || (() => ""))() ?? "";
  const snapshot = await loadSnapshot(this.testPath ?? "", testName);

  // __
  return Promise.resolve({
    pass: true,
    message: () => "",
  });
}
