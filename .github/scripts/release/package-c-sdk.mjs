import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual, parseArgs } from "node:util";
import {
  directoryEntries, libraryName, readArchive, releaseMetadata, sha256,
  validateLibrary, writeArchive,
} from "./package-native.mjs";

export function sdkFilename(version, target, bundle = false) {
  return `tui-test-${bundle ? "with-c-sdk" : "c-sdk"}-${version}-${target}.tar.gz`;
}

function metadata(root, target) {
  const { version, revision, targets } = releaseMetadata(root);
  if (!Object.hasOwn(targets, target)) throw new Error(`Unknown SDK target: ${target}`);
  const header = fs.readFileSync(path.join(root, "crates/tui-test-ffi/include/tui_test.h"));
  const match = header.toString("utf8").match(/^#define ABI_VERSION (\d+)$/m);
  if (!match) throw new Error("Missing ABI_VERSION in the generated C header");
  return { version, revision, target, abiVersion: Number(match[1]) };
}

function seal(entries, manifest) {
  const files = Object.fromEntries([...entries]
    .filter(([name]) => name !== "manifest.json")
    .sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
    .map(([name, data]) => [name, sha256(data)]));
  entries.set("manifest.json", Buffer.from(`${JSON.stringify({ ...manifest, files }, null, 2)}\n`));
  return entries;
}

export function sdkEntries(root, target, library, importLibrary) {
  const manifest = metadata(root, target);
  validateLibrary(library, target);
  const windows = target.includes("windows");
  if (windows && (!importLibrary || !importLibrary.subarray(0, 8).equals(Buffer.from("!<arch>\n")))) {
    throw new Error("Windows SDK requires the matching MSVC import library");
  }
  if (!windows && importLibrary) throw new Error("Import libraries are only supported on Windows");
  const entries = new Map([
    ["include/tui_test.h", fs.readFileSync(path.join(root, "crates/tui-test-ffi/include/tui_test.h"))],
    [`${windows ? "bin" : "lib"}/${libraryName(target)}`, library],
    ["LICENSE", fs.readFileSync(path.join(root, "LICENSE"))],
    ["README.md", fs.readFileSync(path.join(root, "bindings/c/README.md"))],
    ["lib/cmake/tui-test/tui-test-config.cmake", fs.readFileSync(path.join(root, "bindings/c/cmake/tui-test-config.cmake"))],
  ]);
  if (windows) entries.set("lib/tui_test_ffi.lib", importLibrary);
  for (const [name, contents] of directoryEntries(path.join(root, "bindings/c/examples"))) {
    entries.set(`examples/${name}`, contents);
  }
  for (const [asset, license] of [
    ["jetbrains-mono", "OFL.txt"], ["nerd-fonts", "LICENSE"],
    ["xtermjs", "LICENSE"], ...(windows ? [["conpty", "LICENSE"]] : []),
  ]) {
    entries.set(`licenses/${asset}.txt`, fs.readFileSync(path.join(root, "crates/tui-test/assets", asset, license)));
  }
  entries.set("lib/cmake/tui-test/tui-test-config-version.cmake", Buffer.from(
    `set(PACKAGE_VERSION "${manifest.version}")\n`
    + `if(PACKAGE_FIND_VERSION STREQUAL PACKAGE_VERSION)\n`
    + `  set(PACKAGE_VERSION_COMPATIBLE TRUE)\n  set(PACKAGE_VERSION_EXACT TRUE)\nendif()\n`,
  ));
  return seal(entries, manifest);
}

export async function bundleEntries(root, target, sdk, cli) {
  const members = await readArchive(sdk);
  const entries = new Map();
  for (const member of members) {
    if (!["File", "OldFile"].includes(member.type)
        || !/^[a-zA-Z0-9_.+-]+(?:\/[a-zA-Z0-9_.+-]+)*$/.test(member.name)
        || member.name.split("/").some(part => part === "." || part === "..")
        || entries.has(member.name)) {
      throw new Error(`Unexpected SDK archive member: ${member.name}`);
    }
    entries.set(member.name, member.data);
  }
  const manifest = JSON.parse(entries.get("manifest.json")?.toString("utf8") ?? "null");
  const expected = metadata(root, target);
  const sealed = seal(new Map(entries), expected);
  if (!isDeepStrictEqual(manifest, JSON.parse(sealed.get("manifest.json")))) {
    throw new Error("SDK manifest, target, version, revision, or checksum mismatch");
  }
  const windows = target.includes("windows");
  const library = entries.get(`${windows ? "bin" : "lib"}/${libraryName(target)}`);
  if (!library || !isDeepStrictEqual(
    [...entries.keys()].sort(),
    [...sdkEntries(root, target, library, entries.get("lib/tui_test_ffi.lib")).keys()].sort(),
  )) throw new Error("Unexpected SDK contents");
  const executable = `bin/tui-test${windows ? ".exe" : ""}`;
  if (!cli.length) throw new Error("CLI executable is empty");
  entries.set(executable, cli);
  return { entries: seal(entries, expected), executable };
}

function relocatableLibrary(filename, target, output) {
  if (!target.includes("apple")) return fs.readFileSync(filename);
  const scratch = fs.mkdtempSync(path.join(output, ".macos-library-"));
  try {
    const copy = path.join(scratch, libraryName(target));
    fs.copyFileSync(filename, copy);
    execFileSync("install_name_tool", ["-id", `@rpath/${libraryName(target)}`, copy], { stdio: "pipe" });
    execFileSync("codesign", ["--force", "--sign", "-", copy], { stdio: "pipe" });
    return fs.readFileSync(copy);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

async function main() {
  const { values: args } = parseArgs({ options: {
    target: { type: "string" }, library: { type: "string" },
    "import-library": { type: "string" }, sdk: { type: "string" },
    cli: { type: "string" }, output: { type: "string", default: "target/c-sdk-packages" },
  } });
  if (!args.target || (!args.library && !(args.sdk && args.cli))
      || (args.library && (args.sdk || args.cli)) || (args.sdk && args["import-library"])) {
    throw new Error("Usage: package-c-sdk.mjs --target TARGET (--library PATH [--import-library PATH] | --sdk ARCHIVE --cli PATH) [--output DIR]");
  }
  const root = process.cwd();
  const { version } = metadata(root, args.target);
  fs.mkdirSync(args.output, { recursive: true });
  const bundle = Boolean(args.sdk);
  const filename = path.join(args.output, sdkFilename(version, args.target, bundle));
  if (bundle) {
    const { entries, executable } = await bundleEntries(
      root, args.target, fs.readFileSync(args.sdk), fs.readFileSync(args.cli),
    );
    writeArchive(filename, entries, new Set([executable]));
  } else {
    writeArchive(filename, sdkEntries(root, args.target,
      relocatableLibrary(args.library, args.target, args.output),
      args["import-library"] ? fs.readFileSync(args["import-library"]) : undefined,
    ));
  }
  console.log(filename);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) await main();
