import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { run } from "./utils.mjs";

const crateName = "shell-use";
const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const usage = [
  "Usage: node .github/scripts/release/publish-crate.mjs [--dry-run]",
  "",
  "Publishing uses Cargo's configured crates.io credentials.",
].join("\n");

function readPackageVersion() {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--locked", "--no-deps", "--format-version", "1"],
      {
        cwd: repoRoot,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "inherit"],
      },
    ),
  );
  const cratePackage = metadata.packages.find(
    (candidate) => candidate.name === crateName,
  );
  if (!cratePackage) {
    throw new Error(`Could not find the ${crateName} package`);
  }
  return cratePackage.version;
}

async function isPublished(version) {
  const response = await fetch(
    `https://crates.io/api/v1/crates/${encodeURIComponent(crateName)}/${encodeURIComponent(version)}`,
    {
      headers: {
        accept: "application/json",
        "user-agent":
          "shell-use release script (https://github.com/microsoft/shell-use)",
      },
    },
  );

  if (response.status === 404) {
    return false;
  }
  if (!response.ok) {
    const details = (await response.text()).trim();
    throw new Error(
      `Could not check crates.io for ${crateName}@${version}: ${response.status} ${response.statusText}${details ? `: ${details}` : ""}`,
    );
  }
  return true;
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help")) {
    console.log(usage);
    return;
  }

  const unknownArgs = args.filter((argument) => argument !== "--dry-run");
  if (unknownArgs.length > 0) {
    throw new Error(`${usage}\nUnknown argument: ${unknownArgs[0]}`);
  }

  const dryRun = args.includes("--dry-run");
  const version = readPackageVersion();
  const releaseVersion = process.env.RELEASE_TAG?.replace(/^v/, "");
  if (releaseVersion && releaseVersion !== version) {
    throw new Error(
      `RELEASE_TAG has version ${releaseVersion}; ${crateName} has version ${version}`,
    );
  }

  if (!dryRun && (await isPublished(version))) {
    console.log(`${crateName}@${version} is already published`);
    return;
  }

  console.log(
    `${dryRun ? "Verifying" : "Publishing"} ${crateName}@${version}`,
  );
  const cargoArgs = ["publish", "--locked", "-p", crateName];
  if (dryRun) {
    cargoArgs.push("--dry-run");
  }
  run("cargo", cargoArgs, { cwd: repoRoot });
}

await main();
