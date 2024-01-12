import type { MatcherContext, AsyncExpectationResult } from "expect";
import { diffStringsUnified } from "jest-diff";
import path from "node:path";
import process from "node:process";
import fs from "node:fs";
import fsAsync from "node:fs/promises";

import { Terminal } from "../../terminal/term.js";

const snapshots = new Map<string, any>();
const snapshotsIdx = new Map<string, number>();

const snapshotPath = (testPath: string): string => path.join(process.cwd(), path.dirname(testPath), "__snapshots__", `${path.basename(testPath)}.snap.cjs`);

const loadSnapshot = async (testPath: string, testName: string): Promise<string | undefined> => {
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

const writeSnapshot = async (testPath: string, testName: string, snapshot: string): Promise<void> => {
  const snapPath = snapshotPath(testPath);
  if (!fs.existsSync(snapPath)) {
    await fsAsync.mkdir(path.dirname(snapPath), { recursive: true });
  }
  await fsAsync.appendFile(snapPath, `exports[\`${testName}\`] = String.raw\`\n${snapshot}\n\`;`);
};

export async function toMatchSnapshot(this: MatcherContext, terminal: Terminal): AsyncExpectationResult {
  const testName = (this.currentConcurrentTestName || (() => ""))() ?? "";
  const snapshotIdx = snapshotsIdx.get(testName) ?? 0;
  const snapshotPostfixTestName = snapshotIdx != null && snapshotIdx != 0 ? `${testName} ${snapshotIdx}` : testName;
  snapshotsIdx.set(testName, snapshotIdx + 1);
  const snapshot = await loadSnapshot(this.testPath ?? "", snapshotPostfixTestName);

  const { view, shifts } = terminal.serialize();
  const currentSnapshot = `${view}\n${JSON.stringify(shifts, null, 2)}`;

  // TODO: add check for -u flag
  if (snapshot == null) {
    await writeSnapshot(this.testPath ?? "", snapshotPostfixTestName, currentSnapshot);
    return Promise.resolve({
      pass: true,
      message: () => "",
    });
  }
  const pass = currentSnapshot == snapshot;
  return Promise.resolve({
    pass,
    message: pass ? () => "" : () => diffStringsUnified(snapshot ?? "", currentSnapshot),
  });
}
