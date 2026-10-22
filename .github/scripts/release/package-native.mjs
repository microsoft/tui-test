import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual, parseArgs } from "node:util";
import { gzipSync, gunzipSync } from "node:zlib";
import { Header, Parser } from "tar";

export const MAX_MODULE_SIZE = 500 * 1024 * 1024;

export function releaseMetadata(root) {
  const { version, nativeTargets } = JSON.parse(fs.readFileSync(path.join(root, "bindings/js/package.json"), "utf8"));
  if (!/^\d+\.\d+\.\d+(?:-beta\.\d+)?$/.test(version)) {
    throw new Error(`Unsupported release version: ${version}`);
  }
  const revision = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
  return { version, revision, targets: nativeTargets };
}

export function libraryName(target) {
  return target.includes("windows") ? "tui_test_ffi.dll"
    : target.includes("apple") ? "libtui_test_ffi.dylib" : "libtui_test_ffi.so";
}

export function sha256(data) {
  return createHash("sha256").update(data).digest("hex");
}

export function validateLibrary(data, target) {
  const arm = target.startsWith("aarch64-");
  let valid = false;
  if (target.includes("linux")) {
    valid = data.length >= 20 && data.subarray(0, 6).equals(Buffer.from("\x7fELF\x02\x01"))
      && data.readUInt16LE(16) === 3 && data.readUInt16LE(18) === (arm ? 183 : 62);
  } else if (target.includes("apple")) {
    valid = data.length >= 16 && data.readUInt32LE(0) === 0xfeedfacf
      && data.readUInt32LE(4) === (arm ? 0x100000c : 0x1000007) && data.readUInt32LE(12) === 6;
  } else if (data.length >= 64) {
    const offset = data.readUInt32LE(60);
    valid = data.length >= offset + 24 && data.subarray(0, 2).equals(Buffer.from("MZ"))
      && data.subarray(offset, offset + 4).equals(Buffer.from("PE\0\0"))
      && data.readUInt16LE(offset + 4) === 0x8664 && (data.readUInt16LE(offset + 22) & 0x2000) !== 0;
  }
  if (!valid) throw new Error(`Library format or architecture does not match ${target}`);
}

export function archiveBytes(entries) {
  const blocks = [];
  for (const [name, data] of [...entries].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
    const header = new Header({
      path: name, size: data.length, mode: 0o644, uid: 0, gid: 0,
      mtime: new Date(0), type: "File", uname: "", gname: "",
    });
    if (header.encode()) throw new Error(`Archive path requires extended headers: ${name}`);
    blocks.push(header.block, data, Buffer.alloc((512 - data.length % 512) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  return gzipSync(Buffer.concat(blocks));
}

export function writeArchive(filename, entries) {
  const data = archiveBytes(entries);
  fs.writeFileSync(filename, data);
  fs.writeFileSync(`${filename}.sha256`, `${sha256(data)}  ${path.basename(filename)}\n`);
}

export function nativeFilename(version, target) {
  return `tui-test-native-${version}-${target}.tar.gz`;
}

export async function readArchive(data) {
  // Validate the whole gzip stream, including its trailer, before parsing tar.
  const decoded = gunzipSync(data, { maxOutputLength: MAX_MODULE_SIZE + 1024 * 1024 });
  return new Promise((resolve, reject) => {
    const entries = [];
    const parser = new Parser({
      strict: true,
      onReadEntry(entry) {
        const chunks = [];
        entry.on("data", chunk => chunks.push(chunk));
        entry.on("error", reject);
        entry.on("end", () => entries.push({
          name: entry.path, type: entry.type, size: entry.size,
          uid: entry.uid, gid: entry.gid, mtime: entry.mtime, mode: entry.mode,
          data: Buffer.concat(chunks),
        }));
      },
    });
    parser.on("error", reject);
    parser.on("end", () => resolve(entries));
    parser.end(decoded);
  });
}

export async function readNative(data, version, revision, target) {
  const library = libraryName(target);
  const members = await readArchive(data);
  if (members.length !== 3
      || !isDeepStrictEqual(members.map(member => member.name).sort(), [library, "LICENSE", "manifest.json"].sort())
      || members.some(member => !["File", "OldFile"].includes(member.type) || member.size > MAX_MODULE_SIZE)) {
    throw new Error(`Unsafe or unexpected archive members for ${target}`);
  }
  const contents = new Map(members.map(member => [member.name, member.data]));
  const manifest = JSON.parse(contents.get("manifest.json").toString("utf8"));
  const expected = { version, revision, target, library, sha256: sha256(contents.get(library)) };
  if (!isDeepStrictEqual(manifest, expected)) throw new Error(`Native manifest or checksum mismatch for ${target}`);
  validateLibrary(contents.get(library), target);
  return manifest;
}

export async function nativeArchives(root, artifacts, selected) {
  const { version, revision, targets } = releaseMetadata(root);
  const all = selected === undefined;
  selected ??= Object.keys(targets);
  if (!selected.length || new Set(selected).size !== selected.length || selected.some(target => !Object.hasOwn(targets, target))) {
    throw new Error("Select unique, configured native targets");
  }
  const expected = selected.map(target => nativeFilename(version, target)).sort();
  if (all && !isDeepStrictEqual(fs.readdirSync(artifacts).filter(name => name.endsWith(".tar.gz")).sort(), expected)) {
    throw new Error("Expected exactly the seven configured native archives for this release");
  }
  const archives = new Map();
  for (const target of selected) {
    const filename = path.join(artifacts, nativeFilename(version, target));
    const data = fs.readFileSync(filename);
    if (fs.existsSync(`${filename}.sha256`)
        && fs.readFileSync(`${filename}.sha256`, "utf8") !== `${sha256(data)}  ${path.basename(filename)}\n`) {
      throw new Error(`Archive checksum mismatch: ${path.basename(filename)}`);
    }
    archives.set(target, { data, manifest: await readNative(data, version, revision, target) });
  }
  return archives;
}

export function directoryEntries(directory) {
  const entries = new Map();
  function visit(relative) {
    for (const entry of fs.readdirSync(path.join(directory, relative), { withFileTypes: true }).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      const filename = path.join(directory, name);
      if (entry.isSymbolicLink()) throw new Error(`Package contains a symbolic link: ${filename}`);
      if (entry.isDirectory()) visit(name);
      else if (entry.isFile()) entries.set(name, fs.readFileSync(filename));
    }
  }
  visit("");
  return entries;
}

export function checkModuleSize(entries) {
  if ([...entries.values()].reduce((size, data) => size + data.length, 0) >= MAX_MODULE_SIZE) {
    throw new Error("Go module must be smaller than 500 MiB uncompressed");
  }
}

export function moduleEntries(root, archives) {
  const entries = directoryEntries(path.join(root, "bindings/go"));
  for (const name of entries.keys()) {
    if (name.startsWith("internal/native/bundled/") || (name.startsWith("internal/native/bundled_") && name.endsWith(".go"))) {
      throw new Error(`Source module contains generated release files: ${name}`);
    }
  }
  entries.set("LICENSE", fs.readFileSync(path.join(root, "LICENSE")));
  const { targets } = releaseMetadata(root);
  const platforms = new Map();
  for (const [target, { data, manifest }] of [...archives].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
    let [platform, arch] = targets[target].split("-");
    if (platform === "win32") platform = "windows";
    arch = { x64: "amd64", arm64: "arm64" }[arch];
    const key = `${platform}_${arch}`;
    if (!platforms.has(key)) platforms.set(key, []);
    platforms.get(key).push({ target, manifest, platform, arch });
    entries.set(`internal/native/bundled/${target}.tar.gz`, data);
  }
  for (const [key, libraries] of platforms) {
    const { platform, arch } = libraries[0];
    const lines = [
      "// Code generated by package-native.mjs. DO NOT EDIT.",
      `//go:build ${platform} && ${arch}`, "", "package native", "", 'import _ "embed"', "",
    ];
    for (const { target } of libraries) {
      lines.push(`//go:embed bundled/${target}.tar.gz`, `var bundledArchive_${target.replaceAll("-", "_")} []byte`, "");
    }
    lines.push("func init() {");
    for (const { target, manifest } of libraries) {
      lines.push(`\tbundledLibraries["${target}"] = bundledLibrary{`,
        `\t\tname:     "${manifest.library}",`, `\t\tchecksum: "${manifest.sha256}",`,
        `\t\tarchive:  bundledArchive_${target.replaceAll("-", "_")},`, "\t}");
    }
    lines.push("}", "");
    entries.set(`internal/native/bundled_${key}.go`, Buffer.from(lines.join("\n")));
  }
  checkModuleSize(entries);
  return entries;
}

export async function verifyModule(root, directory) {
  const { version, revision, targets } = releaseMetadata(root);
  const archives = new Map();
  for (const target of Object.keys(targets)) {
    const data = fs.readFileSync(path.join(directory, `internal/native/bundled/${target}.tar.gz`));
    archives.set(target, { data, manifest: await readNative(data, version, revision, target) });
  }
  const expected = moduleEntries(root, archives);
  if (!isDeepStrictEqual(directoryEntries(directory), expected)) {
    throw new Error("Go module does not match the source revision and native archives");
  }
  return expected;
}

function resolvedOutput(filename) {
  let ancestor = path.resolve(filename);
  const suffix = [];
  while (!fs.existsSync(ancestor)) {
    suffix.unshift(path.basename(ancestor));
    ancestor = path.dirname(ancestor);
  }
  return path.join(fs.realpathSync(ancestor), ...suffix);
}

async function main() {
  const command = process.argv[2];
  const options = {
    native: { library: { type: "string" }, target: { type: "string" }, output: { type: "string", default: "target/native-packages" } },
    go: { artifacts: { type: "string" }, target: { type: "string", multiple: true }, output: { type: "string", default: "target/go-package" } },
    verify: { package: { type: "string", default: "target/go-package" }, version: { type: "string" } },
  };
  if (!Object.hasOwn(options, command)) throw new Error("Usage: package-native.mjs native|go|verify [options]");
  const { values: args } = parseArgs({ args: process.argv.slice(3), options: options[command] });
  const required = { native: ["library", "target"], go: ["artifacts"], verify: ["version"] }[command];
  for (const key of required) if (!args[key]) throw new Error(`Missing --${key}`);
  const root = process.cwd();
  const { version, revision, targets } = releaseMetadata(root);
  if (command === "native") {
    if (!Object.hasOwn(targets, args.target)) throw new Error(`Unsupported target: ${args.target}`);
    const data = fs.readFileSync(args.library);
    validateLibrary(data, args.target);
    const library = libraryName(args.target);
    const manifest = { version, revision, target: args.target, library, sha256: sha256(data) };
    fs.mkdirSync(args.output, { recursive: true });
    const filename = path.join(args.output, nativeFilename(version, args.target));
    writeArchive(filename, new Map([
      [library, data], ["LICENSE", fs.readFileSync(path.join(root, "LICENSE"))],
      ["manifest.json", Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`)],
    ]));
    console.log(filename);
  } else if (command === "go") {
    const output = resolvedOutput(args.output);
    const source = fs.realpathSync(path.join(root, "bindings/go"));
    if (output === source || output.startsWith(`${source}${path.sep}`)) throw new Error("Output must be outside bindings/go");
    if (fs.existsSync(output)) throw new Error("Output must not exist");
    const entries = moduleEntries(root, await nativeArchives(root, args.artifacts, args.target));
    fs.mkdirSync(output, { recursive: true });
    for (const [name, data] of entries) {
      const filename = path.join(output, name);
      fs.mkdirSync(path.dirname(filename), { recursive: true });
      fs.writeFileSync(filename, data);
    }
    const archive = path.join(path.dirname(output), `tui-test-go-${version}.tar.gz`);
    writeArchive(archive, entries);
    console.log(output);
    console.log(archive);
  } else {
    if (args.version !== version) throw new Error("Release tag does not match the package version");
    await verifyModule(root, args.package);
    console.log(`Verified Go module v${version} from ${revision}`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) await main();
