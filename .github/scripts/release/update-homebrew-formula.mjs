import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const HOMEBREW_ASSETS = [
  "tui-test-aarch64-apple-darwin.tar.gz",
  "tui-test-x86_64-apple-darwin.tar.gz",
  "tui-test-aarch64-unknown-linux-gnu.tar.gz",
  "tui-test-x86_64-unknown-linux-gnu.tar.gz",
];

const RELEASE_TAG_PATTERN = /^\d+\.\d+\.\d+-beta\.\d+$/;
const RELEASE_URL_PATTERN =
  /^[ \t]*url[ \t]+"https:\/\/github\.com\/microsoft\/tui-test\/releases\/download\/[^/"\r\n]+\/([^"\r\n]+)"[ \t]*\r?$/gm;
const VERSION_PATTERN =
  /^([ \t]*version[ \t]+")[^"\r\n]+("[ \t]*\r?)$/gm;

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function hashFile(filePath) {
  return createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

export function updateHomebrewFormula(
  version,
  artifactsDirectory,
  formulaPath,
) {
  if (!RELEASE_TAG_PATTERN.test(version)) {
    throw new Error(
      `Release tag '${version}' must match <major>.<minor>.<patch>-beta.<number>.`,
    );
  }

  const hashes = new Map();
  for (const asset of HOMEBREW_ASSETS) {
    const assetPath = path.resolve(artifactsDirectory, asset);
    if (!fs.existsSync(assetPath) || !fs.statSync(assetPath).isFile()) {
      throw new Error(`Missing release asset: ${assetPath}`);
    }
    hashes.set(asset, hashFile(assetPath));
  }

  const resolvedFormulaPath = path.resolve(formulaPath);
  const formula = fs.readFileSync(resolvedFormulaPath, "utf8");
  const formulaAssets = [...formula.matchAll(RELEASE_URL_PATTERN)].map(
    (match) => match[1],
  );
  const expectedAssets = [...HOMEBREW_ASSETS].sort();
  const uniqueFormulaAssets = [...new Set(formulaAssets)].sort();

  if (
    formulaAssets.length !== HOMEBREW_ASSETS.length ||
    uniqueFormulaAssets.length !== HOMEBREW_ASSETS.length ||
    uniqueFormulaAssets.some((asset, index) => asset !== expectedAssets[index])
  ) {
    throw new Error(
      `Formula release assets must be exactly: ${expectedAssets.join(", ")}`,
    );
  }

  const versionMatches = [...formula.matchAll(VERSION_PATTERN)];
  if (versionMatches.length !== 1) {
    throw new Error(
      `Expected one version declaration in ${resolvedFormulaPath}, found ${versionMatches.length}.`,
    );
  }

  let updatedFormula = formula.replace(
    VERSION_PATTERN,
    (_match, prefix, suffix) => `${prefix}${version}${suffix}`,
  );

  for (const asset of HOMEBREW_ASSETS) {
    const assetBlockPattern = new RegExp(
      `(^[ \\t]*url[ \\t]+"https:\\/\\/github\\.com\\/microsoft\\/tui-test\\/releases\\/download\\/)[^/"\\r\\n]+(\\/${escapeRegExp(asset)}"[ \\t]*\\r?\\n[ \\t]*sha256[ \\t]+")[0-9a-fA-F]{64}("[ \\t]*\\r?$)`,
      "gm",
    );
    const matches = [...updatedFormula.matchAll(assetBlockPattern)];
    if (matches.length !== 1) {
      throw new Error(
        `Expected one URL and checksum block for ${asset}, found ${matches.length}.`,
      );
    }

    updatedFormula = updatedFormula.replace(
      assetBlockPattern,
      (_match, prefix, middle, suffix) =>
        `${prefix}${version}${middle}${hashes.get(asset)}${suffix}`,
    );
  }

  if (updatedFormula !== formula) {
    fs.writeFileSync(resolvedFormulaPath, updatedFormula);
  }

  return {
    changed: updatedFormula !== formula,
    hashes,
  };
}

function main() {
  const [version, artifactsDirectory, formulaPath] = process.argv.slice(2);
  if (!version || !artifactsDirectory || !formulaPath) {
    throw new Error(
      "Usage: node update-homebrew-formula.mjs <version> <artifacts-directory> <formula-path>",
    );
  }

  const result = updateHomebrewFormula(
    version,
    artifactsDirectory,
    formulaPath,
  );
  console.log(
    result.changed
      ? `Updated ${formulaPath} to ${version}.`
      : `${formulaPath} is already at ${version}.`,
  );
  for (const [asset, hash] of result.hashes) {
    console.log(`${hash}  ${asset}`);
  }
}

const invokedPath =
  process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href;
if (import.meta.url === invokedPath) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
