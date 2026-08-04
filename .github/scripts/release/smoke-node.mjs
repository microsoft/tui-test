import fs from "node:fs";
import path from "node:path";
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
  return matches[0].tarball;
}

const rootPackage = findPackage("@microsoft/shell-use");
const platformPackage = findPackage("@microsoft/shell-use-linux-x64-gnu");
const smokeDirectory = path.resolve("smoke");

fs.mkdirSync(smokeDirectory);
runNpm(["init", "-y"], {
  cwd: smokeDirectory,
  stdio: ["ignore", "ignore", "inherit"],
});
runNpm(["install", "--ignore-scripts", platformPackage], {
  cwd: smokeDirectory,
});
runNpm(
  ["install", "--ignore-scripts", "--omit=optional", rootPackage],
  { cwd: smokeDirectory },
);

const requireFromSmoke = createRequire(path.join(smokeDirectory, "package.json"));
const packageEntry = requireFromSmoke.resolve("@microsoft/shell-use");
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
