import fs from "node:fs";

const releaseTag = process.env.RELEASE_TAG;
if (!releaseTag) {
  throw new Error("RELEASE_TAG is required");
}

const expected = releaseTag.replace(/^v/, "");
const jsPackage = JSON.parse(
  fs.readFileSync("bindings/js/package.json", "utf8"),
);
const versions = {
  "Cargo.toml": fs.readFileSync("Cargo.toml", "utf8").match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1],
  "bindings/js/package.json": jsPackage.version,
  "bindings/js/src/version.ts": fs.readFileSync("bindings/js/src/version.ts", "utf8").match(/VERSION\s*=\s*"([^"]+)"/)?.[1],
  "bindings/python/pyproject.toml": fs.readFileSync("bindings/python/pyproject.toml", "utf8").match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1],
  "bindings/python/src/shell_use/_config.py": fs.readFileSync("bindings/python/src/shell_use/_config.py", "utf8").match(/VERSION\s*=\s*"([^"]+)"/)?.[1],
};

for (const [file, version] of Object.entries(versions)) {
  if (version !== expected) {
    throw new Error(`${file} has version ${version}; expected ${expected}`);
  }
}

const nativeLoader = fs.readFileSync("bindings/js/native/index.js", "utf8");
if (!nativeLoader.includes(`'${expected}'`)) {
  throw new Error(
    `bindings/js/native/index.js was not regenerated for ${expected}`,
  );
}
