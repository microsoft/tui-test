import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const bindingsDirectory = path.resolve("bindings/js");
const nativePackagesDirectory = path.join(bindingsDirectory, "npm");
const outputDirectory = path.resolve("package-artifacts/npm");
const packagePath = path.join(bindingsDirectory, "package.json");
const originalPackage = fs.readFileSync(packagePath, "utf8");
const rootPackage = JSON.parse(originalPackage);
rootPackage.optionalDependencies = {};

const runNpm = (args) => execFileSync("npm", args, { stdio: "inherit" });

const targets = Object.values(rootPackage.nativeTargets).sort();

if (targets.length === 0) {
  throw new Error(`No native targets configured in ${packagePath}`);
}

fs.mkdirSync(outputDirectory, { recursive: true });

for (const tag of targets) {
  const [platform, arch, abi] = tag.split("-");
  const extension = { darwin: "dylib", linux: "so", win32: "dll" }[platform];
  if (!extension) throw new Error(`Unsupported native platform: ${platform}`);
  const filename = `tui-test.${tag}.${extension}`;
  const nativePackage = path.join(nativePackagesDirectory, tag);
  const manifestPath = path.join(nativePackage, "package.json");
  const manifest = {
    name: `${rootPackage.name}-${tag}`,
    version: rootPackage.version,
    description: rootPackage.description,
    main: filename,
    files: [filename],
    os: [platform],
    cpu: [arch],
    ...(platform === "linux" ? { libc: [abi === "gnu" ? "glibc" : abi] } : {}),
    license: rootPackage.license,
    author: rootPackage.author,
    repository: rootPackage.repository,
    engines: rootPackage.engines,
    publishConfig: rootPackage.publishConfig,
  };
  const artifact = path.join(bindingsDirectory, "artifacts", filename);
  if (!fs.existsSync(artifact)) {
    throw new Error(`Missing shared library for ${manifest.name}: ${artifact}`);
  }
  fs.mkdirSync(nativePackage, { recursive: true });
  fs.copyFileSync(artifact, path.join(nativePackage, filename));
  rootPackage.optionalDependencies[manifest.name] = rootPackage.version;
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  fs.writeFileSync(path.join(nativePackage, "README.md"), `# ${manifest.name}\n\nThis package contains the tui-test shared library for ${tag}.\n`);
  runNpm([
    "pack",
    nativePackage,
    "--ignore-scripts",
    "--pack-destination",
    outputDirectory,
  ]);
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
