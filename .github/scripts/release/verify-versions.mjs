import fs from "node:fs";

const releaseTag = process.env.RELEASE_TAG;
if (!releaseTag) {
  throw new Error("RELEASE_TAG is required");
}

const expected = releaseTag.replace(/^v/, "");

function read(file) {
  return fs.readFileSync(file, "utf8");
}

function matchedVersion(file, pattern) {
  const version = read(file).match(pattern)?.[1];
  if (!version) {
    throw new Error(`Could not read a version from ${file}`);
  }
  return version;
}

const jsPackage = JSON.parse(read("bindings/js/package.json"));
const jsPackageLock = JSON.parse(read("bindings/js/package-lock.json"));
const versions = {
  "Cargo.toml [workspace.package]": matchedVersion(
    "Cargo.toml",
    /^\[workspace\.package\]\s*$[\s\S]*?^\s*version\s*=\s*"([^"]+)"/m,
  ),
  "bindings/js/package.json": jsPackage.version,
  "bindings/js/package-lock.json": jsPackageLock.version,
  "bindings/js/package-lock.json packages['']":
    jsPackageLock.packages?.[""]?.version,
  "bindings/js/src/version.ts": matchedVersion(
    "bindings/js/src/version.ts",
    /VERSION\s*=\s*"([^"]+)"/,
  ),
  "bindings/python/pyproject.toml": matchedVersion(
    "bindings/python/pyproject.toml",
    /^\s*version\s*=\s*"([^"]+)"/m,
  ),
  "bindings/python/src/shell_use/_config.py": matchedVersion(
    "bindings/python/src/shell_use/_config.py",
    /VERSION\s*=\s*"([^"]+)"/,
  ),
};

for (const [file, version] of Object.entries(versions)) {
  if (version !== expected) {
    throw new Error(`${file} has version ${version}; expected ${expected}`);
  }
}

const workspaceVersionManifests = [
  "crates/shell-use/Cargo.toml",
  "crates/shell-use-cli/Cargo.toml",
  "bindings/js/Cargo.toml",
  "bindings/python/native/Cargo.toml",
];
for (const file of workspaceVersionManifests) {
  if (!/^\s*version\.workspace\s*=\s*true\s*$/m.test(read(file))) {
    throw new Error(`${file} must inherit workspace.package.version`);
  }
}

const nativeLoader = read("bindings/js/native/index.js");
const loaderVersions = new Set(
  [...nativeLoader.matchAll(/bindingPackageVersion !== '([^']+)'/g)].map(
    ([, version]) => version,
  ),
);
if (loaderVersions.size !== 1 || !loaderVersions.has(expected)) {
  throw new Error(
    `bindings/js/native/index.js has package versions ${[...loaderVersions].join(", ") || "none"}; expected ${expected}`,
  );
}

console.log(
  `Verified ${expected} across release metadata; Rust packages inherit the workspace version.`,
);
