// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import os from "node:os";
import fs from "node:fs";
import path from "node:path";
import url from "node:url";
import which from "which";
import fsAsync from "node:fs/promises";

export enum Shell {
  Bash = "bash",
  WindowsPowershell = "powershell",
  Powershell = "pwsh",
  Cmd = "cmd",
  Fish = "fish",
  Zsh = "zsh",
}

export const defaultShell = os.platform() == "win32" ? Shell.Cmd : Shell.Bash;
export const userZdotdir = process.env?.ZDOTDIR ?? os.homedir() ?? `~`;
export const zdotdir = path.join(os.tmpdir(), `tact-zsh`);

export const shellLaunch = async (shell: Shell) => {
  const platform = os.platform();
  const shellTarget = shell == Shell.Bash && platform == "win32" ? await gitBashPath() : platform == "win32" ? `${shell}.exe` : shell;
  const shellFolderPath = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "..", "..", "shell");
  let shellArgs: string[] | undefined = undefined;

  switch (shell) {
    case Shell.Bash:
      shellArgs = ["--init-file", path.join(shellFolderPath, "shellIntegration.bash")];
      break;
    case Shell.WindowsPowershell:
      shellArgs = ["-NoLogo", "-noexit", "-command", `try { . "${path.join(shellFolderPath, "shellIntegration.ps1")}" } catch {}`];
      break;
    case Shell.Powershell:
      shellArgs = ["-noexit", "-command", `. "${path.join(shellFolderPath, "shellIntegration.ps1")}"`];
      break;
    case Shell.Fish:
      shellArgs = ["--init-command", `. ${path.join(shellFolderPath, "shellIntegration.fish").replace(/(\s+)/g, "\\$1")}`];
      break;
  }

  return { shellTarget, shellArgs };
};

export const shellEnv = (shell: Shell) => {
  const env = {
    ...process.env,
  };
  switch (shell) {
    case Shell.Cmd: {
      return { ...env, PROMPT: "$G" };
    }
    case Shell.Zsh: {
      return { ...env, ZDOTDIR: zdotdir, USER_ZDOTDIR: userZdotdir };
    }
  }
  return env;
};

export const setupZshDotfiles = async () => {
  const shellFolderPath = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "..", "..", "shell");
  await fsAsync.cp(path.join(shellFolderPath, "shellIntegration-rc.zsh"), path.join(zdotdir, ".zshrc"));
  await fsAsync.cp(path.join(shellFolderPath, "shellIntegration-profile.zsh"), path.join(zdotdir, ".zprofile"));
  await fsAsync.cp(path.join(shellFolderPath, "shellIntegration-env.zsh"), path.join(zdotdir, ".zshenv"));
  await fsAsync.cp(path.join(shellFolderPath, "shellIntegration-login.zsh"), path.join(zdotdir, ".zlogin"));
};

const gitBashPath = async (): Promise<string> => {
  const gitBashPaths = await getGitBashPaths();
  for (const gitBashPath of gitBashPaths) {
    if (fs.existsSync(gitBashPath)) {
      return gitBashPath;
    }
  }
  throw new Error("unable to find a git bash executable installed");
};

const getGitBashPaths = async (): Promise<string[]> => {
  const gitDirs: Set<string> = new Set();

  const gitExePath = await which("git.exe", { nothrow: true });
  if (gitExePath) {
    const gitExeDir = path.dirname(gitExePath);
    gitDirs.add(path.resolve(gitExeDir, "../.."));
  }

  const addValid = <T>(set: Set<T>, value: T | undefined) => {
    if (value) set.add(value);
  };

  // Add common git install locations
  addValid(gitDirs, process.env["ProgramW6432"]);
  addValid(gitDirs, process.env["ProgramFiles"]);
  addValid(gitDirs, process.env["ProgramFiles(X86)"]);
  addValid(gitDirs, `${process.env["LocalAppData"]}\\Program`);

  const gitBashPaths: string[] = [];
  for (const gitDir of gitDirs) {
    gitBashPaths.push(
      `${gitDir}\\Git\\bin\\bash.exe`,
      `${gitDir}\\Git\\usr\\bin\\bash.exe`,
      `${gitDir}\\usr\\bin\\bash.exe`, // using Git for Windows SDK
    );
  }

  // Add special installs that don't follow the standard directory structure
  gitBashPaths.push(`${process.env["UserProfile"]}\\scoop\\apps\\git\\current\\bin\\bash.exe`);
  gitBashPaths.push(`${process.env["UserProfile"]}\\scoop\\apps\\git-with-openssh\\current\\bin\\bash.exe`);

  return gitBashPaths;
};
