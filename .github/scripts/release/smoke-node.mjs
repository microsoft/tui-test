import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

import {
  listPackageTarballs,
  readPackageManifest,
  runNpm,
} from "./utils.mjs";

const packages = listPackageTarballs("npm-packages").map((tarball) => ({
  tarball,
  manifest: readPackageManifest(tarball),
}));

function findPackage(name) {
  const matches = packages.filter(({ manifest }) => manifest.name === name);
  if (matches.length !== 1) {
    throw new Error(`Expected one ${name} package, found ${matches.length}`);
  }
  return matches[0];
}

const rootPackage = findPackage("@microsoft/shell-use");
const platformPackage = findPackage("@microsoft/shell-use-linux-x64-gnu");
const smokeDirectory = path.resolve("smoke");

for (const { manifest } of [rootPackage, platformPackage]) {
  if (manifest.bin !== undefined) {
    throw new Error(`${manifest.name} unexpectedly declares a CLI executable`);
  }
}

fs.mkdirSync(smokeDirectory, { recursive: true });
runNpm(["init", "-y"], {
  cwd: smokeDirectory,
  stdio: ["ignore", "ignore", "inherit"],
});
runNpm(["install", "--ignore-scripts", platformPackage.tarball], {
  cwd: smokeDirectory,
});
runNpm(
  [
    "install",
    "--ignore-scripts",
    "--omit=optional",
    rootPackage.tarball,
  ],
  { cwd: smokeDirectory },
);

process.env.SHELL_USE_BIN = path.join(smokeDirectory, "missing-shell-use");
if (process.platform !== "win32") {
  process.env.PATH = "/usr/bin:/bin";
}
const cliProbe = spawnSync("shell-use", ["--version"], { stdio: "ignore" });
if (!cliProbe.error || cliProbe.error.code !== "ENOENT") {
  throw new Error("shell-use CLI unexpectedly available in smoke PATH");
}

const requireFromSmoke = createRequire(path.join(smokeDirectory, "package.json"));
const packageEntry = requireFromSmoke.resolve(rootPackage.manifest.name);
const { ShellUse } = await import(pathToFileURL(packageEntry).href);
const session = ShellUse.ephemeral("release-smoke");
try {
  await session.open();
  await session.submit("echo release-smoke");
  await session.waitCommand();
  await session.expectText("release-smoke", { strict: false });
} finally {
  await session.closeQuiet();
}
