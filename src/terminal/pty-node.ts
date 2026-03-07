// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import pty from "node-pty";
import process from "node:process";
import { EventEmitter } from "node:events";

import type { IPtyBackend, PtyOptions } from "./pty.js";

export const createNodePty = (
  target: string,
  args: string[],
  options: PtyOptions
): IPtyBackend => {
  const handle = pty.spawn(target, args, {
    name: "xterm-256color",
    cols: options.cols,
    rows: options.rows,
    cwd: options.cwd,
    env: options.env,
  });

  // On Windows, node-pty's WindowsTerminal extends EventEmitter and its
  // outSocket error handler throws non-EIO errors when
  // `this.listeners('error').length < 2`. Adding error listeners prevents
  // the throw from crashing the worker when a child process exits quickly.
  const emitter = handle as unknown as EventEmitter;
  if (typeof emitter.on === "function") {
    emitter.on("error", () => {});
    emitter.on("error", () => {});
  }

  return {
    get pid() {
      return handle.pid;
    },
    onData(callback: (data: string) => void) {
      handle.onData(callback);
    },
    onExit(callback: (exit: { exitCode: number; signal?: number }) => void) {
      handle.onExit(callback);
    },
    write(data: string) {
      try {
        handle.write(data);
      } catch {
        // pty may have closed between the exited check and the write call
      }
    },
    resize(cols: number, rows: number) {
      try {
        handle.resize(cols, rows);
      } catch {
        // pty may have closed between the exited check and the resize call
      }
    },
    kill() {
      try {
        process.kill(handle.pid, 9);
      } catch {
        // process may have already exited
      }
    },
  };
};
