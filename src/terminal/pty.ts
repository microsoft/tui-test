// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { isBunPtySupported } from "../utils/runtime.js";

export type PtyOptions = {
  cols: number;
  rows: number;
  cwd: string;
  env?: { [key: string]: string | undefined };
};

export interface IPtyBackend {
  readonly pid: number;
  onData(callback: (data: string) => void): void;
  onExit(callback: (exit: { exitCode: number; signal?: number }) => void): void;
  write(data: string): void;
  resize(cols: number, rows: number): void;
  kill(): void;
}

export const createPty = async (
  target: string,
  args: string[],
  options: PtyOptions
): Promise<IPtyBackend> => {
  if (isBunPtySupported()) {
    const { createBunPty } = await import("./pty-bun.js");
    return createBunPty(target, args, options);
  }
  const { createNodePty } = await import("./pty-node.js");
  return createNodePty(target, args, options);
};
