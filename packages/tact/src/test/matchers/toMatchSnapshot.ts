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
  await fsAsync.appendFile(snapPath, `exports[\`${testName}\`] = String.raw\`\n${snapshot}\n\`;\n\n`);
};

const generateSnapshot = (terminal: Terminal) => {
  const { view, shifts } = terminal.serialize();
  if (shifts.size === 0) {
    return view;
  }
  return `${view}\n${JSON.stringify(Object.fromEntries(shifts), null, 2)}`;
};

export async function toMatchSnapshot(this: MatcherContext, terminal: Terminal): AsyncExpectationResult {
  const testName = (this.currentConcurrentTestName || (() => ""))() ?? "";
  const snapshotIdx = snapshotsIdx.get(testName) ?? 0;
  const snapshotPostfixTestName = snapshotIdx != null && snapshotIdx != 0 ? `${testName} ${snapshotIdx}` : testName;
  snapshotsIdx.set(testName, snapshotIdx + 1);
  const existingSnapshot = await loadSnapshot(this.testPath ?? "", snapshotPostfixTestName);
  const newSnapshot = generateSnapshot(terminal);

  if (existingSnapshot == null || globalThis.__expectState.updateSnapshot) {
    await writeSnapshot(this.testPath ?? "", snapshotPostfixTestName, newSnapshot);
    return Promise.resolve({
      pass: true,
      message: () => "",
    });
  }
  const pass = existingSnapshot == newSnapshot;
  return Promise.resolve({
    pass,
    message: pass ? () => "" : () => diffStringsUnified(existingSnapshot, newSnapshot ?? ""),
  });
}
