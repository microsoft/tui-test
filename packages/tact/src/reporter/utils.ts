// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { promisify } from "node:util";
import { exec } from "node:child_process";

import { Shell, shellLaunch } from "../terminal/shell.js";

const execAsync = promisify(exec);
const ansiRegex = new RegExp(
  // eslint-disable-next-line no-control-regex
  "([\\u001B\\u009B][[\\]()#;?]*(?:(?:(?:[a-zA-Z\\d]*(?:;[-a-zA-Z\\d\\/#&.:=?%@~_]*)*)?\\u0007)|(?:(?:\\d{1,4}(?:;\\d{0,4})*)?[\\dA-PR-TZcf-ntqry=><~])))",
  "g",
);

export const loadShellVersions = async (shell: Shell[]): Promise<{ shell: Shell; version?: string; target: string }[]> => {
  return Promise.all(
    shell.map(async (shell) => {
      const { shellTarget: target } = await shellLaunch(shell);
      let version: string | undefined;
      try {
        switch (shell) {
          case Shell.Bash: {
            const output = (await execAsync('echo "$BASH_VERSION"', { shell: target })).stdout;
            if (output.trim().length != 0) {
              version = output.trim();
            }
            break;
          }
          case (Shell.WindowsPowershell, Shell.Powershell): {
            const output = (await execAsync("$PSVersionTable", { shell: target })).stdout;
            const match = output.match(/PSVersion\s*([^\s]*)/)?.at(1);
            if (match != null) {
              version = match;
            }
            break;
          }
          case Shell.Cmd: {
            const output = (await execAsync("ver", { shell: target })).stdout;
            const match = output.match(/\[Version\s*(.*)\]/)?.at(1);
            if (match != null) {
              version = match;
            }
            break;
          }
          case Shell.Fish: {
            const output = (await execAsync('echo "$version"', { shell: target })).stdout;
            if (output.trim().length != 0) {
              version = output.trim();
            }
            break;
          }
          case Shell.Zsh: {
            const output = (await execAsync('echo "$ZSH_VERSION"', { shell: target })).stdout;
            if (output.trim().length != 0) {
              version = output.trim();
            }
            break;
          }
        }
      } catch {
        /* empty */
      }
      version = (version?.length ?? 0) > 0 ? version : undefined;
      return { shell, target, version };
    }),
  );
};

export function stripAnsiEscapes(str: string): string {
  return str.replace(ansiRegex, "");
}

export function fitToWidth(line: string, width: number, prefix?: string): string {
  const prefixLength = prefix ? stripAnsiEscapes(prefix).length : 0;
  width -= prefixLength;
  if (line.length <= width) return line;

  // Even items are plain text, odd items are control sequences.
  const parts = line.split(ansiRegex);
  const taken: string[] = [];
  for (let i = parts.length - 1; i >= 0; i--) {
    if (i % 2) {
      // Include all control sequences to preserve formatting.
      taken.push(parts[i]);
    } else {
      let part = parts[i].substring(parts[i].length - width);
      if (part.length < parts[i].length && part.length > 0) {
        // Add ellipsis if we are truncating.
        part = "\u2026" + part.substring(1);
      }
      taken.push(part);
      width -= part.length;
    }
  }
  return taken.reverse().join("");
}

export const ansi = {
  eraseCurrentLine: "\x1b[2K\x1b[G", // clear line and move cursor to start
  cursorPreviousLine: "\x1b[F",
  cursorNextLine: "\x1b[E",
};
