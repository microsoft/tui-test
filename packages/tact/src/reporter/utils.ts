import { Shell, shellTarget } from "../terminal/shell.js";
import { promisify } from "node:util";
import { exec } from "node:child_process";
import { Suite } from "../test/suite.js";
import chalk from "chalk";

const execAsync = promisify(exec);
const ansiRegex = new RegExp(
  "([\\u001B\\u009B][[\\]()#;?]*(?:(?:(?:[a-zA-Z\\d]*(?:;[-a-zA-Z\\d\\/#&.:=?%@~_]*)*)?\\u0007)|(?:(?:\\d{1,4}(?:;\\d{0,4})*)?[\\dA-PR-TZcf-ntqry=><~])))",
  "g"
);

export const loadShellVersions = async (shell: Shell[]): Promise<{ shell: Shell; version?: string; target: string }[]> => {
  return Promise.all(
    shell.map(async (shell) => {
      const target = await shellTarget(shell);
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
      } catch {}
      version = (version?.length ?? 0) > 0 ? version : undefined;
      return { shell, target, version };
    })
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

// TODO: finish summary
export const getSummary = (allSuites: Suite[]) => {
  const suites = [...allSuites];

  // 1) [setup] › src\tests\playwright\auth.pwt.setup.ts:9:6 ›  ───────────────────────────────────────
  // Retry #1 ───────────────────────────────────────────────────────────────────────────────────────
  // ───────── stdout ─────────
  // ───────── stderr ─────────
  const maxWidth = process.stdout.columns;
  let currentTest = 1;
  while (suites != null) {
    const suite = suites.shift();
    for (const test of suite?.tests ?? []) {
      for (let idx = 0; idx < test.results.length; idx++) {
        const result = test.results[idx];
        const resultColor = chalk.green;
        const title = idx == 0 ? resultColor(`  ) `) : chalk.gray(`    Retry #${idx} `.padEnd(maxWidth - 4, "─"));
      }
      currentTest += 1;
    }
  }
};

/*
3 failed
    [setup] › src\tests\playwright\auth.pwt.setup.ts:9:6 ›  ────────────────────────────────────────    
    [setup] › src\tests\playwright\auth.pwt.setup.ts:57:6 › authenticate in Lxp as non AAD user ────    
    [setup] › src\tests\playwright\auth.pwt.setup.ts:113:6 › authenticate as non AAD user ──────────    
22 skipped
*/
