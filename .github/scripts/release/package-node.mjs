import fs from "node:fs";
import path from "node:path";

import { runNpm } from "./utils.mjs";

const bindingsDirectory = path.resolve("bindings/js");
const nativePackagesDirectory = path.join(bindingsDirectory, "npm");
const outputDirectory = path.resolve("package-artifacts/npm");
const packagePath = path.join(bindingsDirectory, "package.json");

const nativePackages = fs
  .readdirSync(nativePackagesDirectory, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => path.join(nativePackagesDirectory, entry.name))
  .sort();

if (nativePackages.length === 0) {
  throw new Error(`No native packages found in ${nativePackagesDirectory}`);
}

fs.mkdirSync(outputDirectory, { recursive: true });

for (const nativePackage of nativePackages) {
  const hasNativeAddon = fs
    .readdirSync(nativePackage, { withFileTypes: true })
    .some((entry) => entry.isFile() && entry.name.endsWith(".node"));
  if (!hasNativeAddon) {
    throw new Error(`Missing native addon in ${nativePackage}`);
  }

  runNpm([
    "pack",
    nativePackage,
    "--pack-destination",
    outputDirectory,
  ]);
}

const originalPackage = fs.readFileSync(packagePath, "utf8");
const rootPackage = JSON.parse(originalPackage);
rootPackage.optionalDependencies = {};

for (const nativePackage of nativePackages) {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(nativePackage, "package.json"), "utf8"),
  );
  rootPackage.optionalDependencies[manifest.name] = manifest.version;
}

try {
  fs.writeFileSync(
    packagePath,
    `${JSON.stringify(rootPackage, null, 2)}\n`,
  );
  runNpm([
    "pack",
    bindingsDirectory,
    "--ignore-scripts",
    "--pack-destination",
    outputDirectory,
  ]);
} finally {
  fs.writeFileSync(packagePath, originalPackage);
}
