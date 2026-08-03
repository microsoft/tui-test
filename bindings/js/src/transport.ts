import net from "node:net";
import { spawn } from "node:child_process";

import { socketPath } from "./config.js";
import { DaemonError } from "./errors.js";
import type { Response } from "./types.js";

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function spawnDaemon(
  binary: string,
  args: string[],
  env: NodeJS.ProcessEnv,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { env, stdio: "ignore", windowsHide: true });
    child.once("error", reject);
    child.once("exit", () => resolve());
  });
}

function connect(target: string): Promise<net.Socket> {
  return new Promise((resolve, reject) => {
    const sock = new net.Socket();
    const onError = (err: Error) => {
      sock.destroy();
      reject(err);
    };
    sock.once("error", onError);
    sock.once("connect", () => {
      sock.removeListener("error", onError);
      resolve(sock);
    });
    sock.connect(target);
  });
}

export async function canConnect(session: string, home?: string): Promise<boolean> {
  try {
    const sock = await connect(socketPath(session, home));
    sock.destroy();
    return true;
  } catch {
    return false;
  }
}

export async function ensureDaemon(
  session: string,
  home: string | undefined,
  binary: string,
): Promise<void> {
  if (await canConnect(session, home)) {
    return;
  }
  const env = { ...process.env };
  if (home) {
    env.SHELL_USE_HOME = home;
  }
  try {
    await spawnDaemon(binary, ["--session", session, "daemon", "start"], env);
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") {
      throw new DaemonError(
        `could not find the '${binary}' binary on PATH; set SHELL_USE_BIN or pass { binary }`,
      );
    }
    throw new DaemonError(`failed to start daemon: ${(err as Error).message}`);
  }
  for (let i = 0; i < 100; i++) {
    if (await canConnect(session, home)) {
      return;
    }
    await delay(50);
  }
  throw new DaemonError(`daemon for session '${session}' did not become ready`);
}

export async function request(
  session: string,
  home: string | undefined,
  binary: string,
  payload: unknown,
  autostart = true,
): Promise<Response> {
  if (autostart) {
    await ensureDaemon(session, home, binary);
  }
  let sock: net.Socket;
  try {
    sock = await connect(socketPath(session, home));
  } catch (err) {
    throw new DaemonError(
      `could not connect to session '${session}': ${(err as Error).message}`,
    );
  }
  return new Promise<Response>((resolve, reject) => {
    let buf = "";
    let settled = false;
    sock.setEncoding("utf8");
    sock.on("data", (chunk: string) => {
      buf += chunk;
      const nl = buf.indexOf("\n");
      if (nl >= 0 && !settled) {
        settled = true;
        sock.destroy();
        try {
          resolve(JSON.parse(buf.slice(0, nl)) as Response);
        } catch (err) {
          reject(new DaemonError(`invalid response from daemon: ${(err as Error).message}`));
        }
      }
    });
    sock.on("error", (err) => {
      if (!settled) {
        settled = true;
        reject(new DaemonError((err as Error).message));
      }
    });
    sock.on("close", () => {
      if (!settled) {
        settled = true;
        reject(new DaemonError("daemon closed the connection without responding"));
      }
    });
    sock.write(JSON.stringify(payload) + "\n");
  });
}
