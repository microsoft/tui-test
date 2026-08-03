import { rmSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

let sessionCounter = 0;

export function uniqueSession(prefix?: string): string {
  const sanitized = (prefix ?? "shell-use").replace(/[^A-Za-z0-9_-]/g, "-");
  const random = (Math.random().toString(36).slice(2) + "0").slice(0, 8);
  const suffix = `-${process.pid}-${random}-${sessionCounter++}`;
  const room = Math.max(1, 64 - suffix.length);
  return `${sanitized.slice(0, room)}${suffix}`;
}

const tempHomes = new Set<string>();
let sweeperRegistered = false;

function registerTempHomeSweeper(): void {
  if (sweeperRegistered) {
    return;
  }
  sweeperRegistered = true;
  process.on("exit", () => {
    for (const dir of tempHomes) {
      try {
        rmSync(dir, { recursive: true, force: true });
      } catch {
        /* best effort */
      }
    }
  });
}

export async function createTempHome(): Promise<string> {
  const dir = await mkdtemp(path.join(os.tmpdir(), "shell-use-"));
  tempHomes.add(dir);
  registerTempHomeSweeper();
  return dir;
}

export async function removeTempHome(dir: string): Promise<void> {
  await rm(dir, { recursive: true, force: true });
  tempHomes.delete(dir);
}
