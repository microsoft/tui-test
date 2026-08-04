import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

export function run(command, args, options = {}) {
  const { stdio = "inherit", ...rest } = options;
  execFileSync(command, args, { ...rest, stdio });
}

function npmInvocation(args) {
  if (process.platform === "win32") {
    return {
      command: process.env.ComSpec ?? "cmd.exe",
      args: ["/d", "/s", "/c", "npm", ...args],
    };
  }
  return { command: "npm", args };
}

export function runNpm(args, options = {}) {
  const invocation = npmInvocation(args);
  run(invocation.command, invocation.args, options);
}

export function spawnNpm(args, options = {}) {
  const invocation = npmInvocation(args);
  return spawnSync(invocation.command, invocation.args, options);
}

export function listPackageTarballs(directory) {
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".tgz"))
    .map((entry) => path.resolve(directory, entry.name))
    .sort();
}

export function readPackageManifest(tarball) {
  const contents = execFileSync(
    "tar",
    ["-xOf", tarball, "package/package.json"],
    { encoding: "utf8" },
  );
  return JSON.parse(contents);
}
