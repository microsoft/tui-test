import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const bindingsDirectory = path.resolve("bindings/js");
const nativePackagesDirectory = path.join(bindingsDirectory, "npm");
const outputDirectory = path.resolve("package-artifacts/npm");
const packagePath = path.join(bindingsDirectory, "package.json");

const runNpm = (args) => execFileSync("npm", args, { stdio: "inherit" });

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

  const platform = path.basename(nativePackage);
  const library = platform.startsWith("win32-")
    ? `tui_test_ffi.${platform}.dll`
    : `libtui_test_ffi.${platform}.${platform.startsWith("darwin-") ? "dylib" : "so"}`;
  const source = path.join(bindingsDirectory, "artifacts", library);
  if (!fs.existsSync(source)) {
    throw new Error(`Missing shared engine in ${source}`);
  }
  fs.copyFileSync(source, path.join(nativePackage, library));
  const manifestPath = path.join(nativePackage, "package.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.files !== undefined) {
    manifest.files = [...new Set([...manifest.files, library])];
  }
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

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
