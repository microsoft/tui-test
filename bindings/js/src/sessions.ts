import { readFile, readdir } from "node:fs/promises";

import { homeDir, recordingPath, resolveBinary, resolveHome, resolveSession } from "./config.js";
import { NoSessionError } from "./errors.js";
import { unwrap } from "./protocol.js";
import * as transport from "./transport.js";
import type { DaemonStatus, HomeOptions } from "./types.js";

export async function sessions(opts: { home?: string } = {}): Promise<string[]> {
  const home = resolveHome(opts.home);
  const dir = homeDir(home);
  const out: string[] = [];
  let entries: string[];
  try {
    entries = await readdir(dir);
  } catch {
    return out;
  }
  for (const entry of entries.sort()) {
    if (entry.endsWith(".pid")) {
      const name = entry.slice(0, -4);
      if (await transport.canConnect(name, home)) {
        out.push(name);
      }
    }
  }
  return out;
}

export async function closeAll(opts: HomeOptions = {}): Promise<void> {
  const home = resolveHome(opts.home);
  const binary = resolveBinary(opts.binary);
  for (const name of await sessions({ home })) {
    try {
      await transport.request(name, home, binary, { kind: "close" }, false);
    } catch {
      /* best effort */
    }
  }
}

export async function daemonStatus(
  session?: string,
  opts: HomeOptions = {},
): Promise<DaemonStatus> {
  const s = resolveSession(session);
  const home = resolveHome(opts.home);
  const binary = resolveBinary(opts.binary);
  return unwrap(await transport.request(s, home, binary, { kind: "status" })) as DaemonStatus;
}

export async function daemonStop(session?: string, opts: HomeOptions = {}): Promise<void> {
  const s = resolveSession(session);
  const home = resolveHome(opts.home);
  const binary = resolveBinary(opts.binary);
  if (!(await transport.canConnect(s, home))) {
    return;
  }
  unwrap(await transport.request(s, home, binary, { kind: "shutdown" }, false));
}

export async function getRecording(
  session?: string,
  opts: { home?: string } = {},
): Promise<string> {
  const s = resolveSession(session);
  const home = resolveHome(opts.home);
  try {
    return await readFile(recordingPath(s, home), "utf8");
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") {
      throw new NoSessionError(`no recording for session '${s}'`);
    }
    throw err;
  }
}
