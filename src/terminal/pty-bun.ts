// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { EventEmitter } from "node:events";

import type { IPtyBackend, PtyOptions } from "./pty.js";

export const createBunPty = (
  target: string,
  args: string[],
  options: PtyOptions
): IPtyBackend => {
  const emitter = new EventEmitter();

  const terminal = new Bun.Terminal({
    cols: options.cols,
    rows: options.rows,
    data(_term: unknown, data: Uint8Array) {
      emitter.emit("data", new TextDecoder().decode(data));
    },
  });
  const proc = Bun.spawn([target, ...args], {
    cwd: options.cwd,
    env: options.env as Record<string, string>,
    terminal: terminal
  });

  proc.exited.then((exitCode: number) => {
    emitter.emit("exit", { exitCode, signal: proc.signalCode ?? undefined });
  });

  return {
    get pid() {
      return proc.pid;
    },
    onData(callback: (data: string) => void) {
      emitter.on("data", callback);
    },
    onExit(callback: (exit: { exitCode: number; signal?: number }) => void) {
      emitter.on("exit", callback);
    },
    write(data: string) {
      terminal.write(data);
    },
    resize(cols: number, rows: number) {
      terminal.resize(cols, rows);
    },
    kill() {
      proc.kill(9);
    },
  };
};
