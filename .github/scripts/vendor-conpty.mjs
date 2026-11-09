import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { gzipSync } from "node:zlib";
import { unzipSync } from "fflate";

const PACKAGE = "Microsoft.Windows.Console.ConPTY";
const ASSETS = fileURLToPath(new URL("../../crates/tui-test/assets/conpty/", import.meta.url));
const SIGNATURE = Buffer.from([0xbd, 0x04, 0xef, 0xfe, 0, 0, 1, 0]);

export function packageUrl(version) {
  return `https://www.nuget.org/api/v2/package/${PACKAGE}/${encodeURIComponent(version)}`;
}

function fileVersion(data) {
  const offset = data.indexOf(SIGNATURE);
  if (offset < 0 || offset + 16 > data.length || data.indexOf(SIGNATURE, offset + 1) !== -1) {
    throw new Error("Expected one VS_FIXEDFILEINFO resource");
  }
  const ms = data.readUInt32LE(offset + 8);
  const ls = data.readUInt32LE(offset + 12);
  return `${ms >>> 16}.${ms & 0xffff}.${ls >>> 16}.${ls & 0xffff}`;
}

function validatePin(version, sha256) {
  if (!/^\d+(?:\.\d+)+(?:-[a-zA-Z0-9.-]+)?$/.test(version)) {
    throw new Error("Version must contain dot-separated integers and an optional prerelease label");
  }
  if (!/^[0-9a-f]{64}$/.test(sha256)) {
    throw new Error("SHA256 must be 64 lowercase hexadecimal characters");
  }
}

export function prepareConptyAssets(packageBytes, { version, sha256 }) {
  validatePin(version, sha256);
  if (createHash("sha256").update(packageBytes).digest("hex") !== sha256) {
    throw new Error("NuGet package checksum mismatch");
  }
  const archive = unzipSync(packageBytes);
  const entries = [];
  const versions = new Set();
  const files = new Map();
  for (const [architecture, machine] of [["x86", 0x14c], ["x64", 0x8664], ["arm64", 0xaa64]]) {
    const dll = [
      `runtimes/win-${architecture}/native/conpty.dll`,
      `runtimes/win10-${architecture}/native/conpty.dll`,
    ].find(name => archive[name]);
    if (!dll) throw new Error(`Missing NuGet asset: conpty.dll for ${architecture}`);
    for (const [filename, source] of [
      ["conpty.dll", dll],
      ["OpenConsole.exe", `build/native/runtimes/${architecture}/OpenConsole.exe`],
    ]) {
      if (!archive[source]) throw new Error(`Missing NuGet asset: ${source}`);
      const data = Buffer.from(archive[source]);
      if (data.length < 64 || data.toString("ascii", 0, 2) !== "MZ") {
        throw new Error(`Invalid PE header for ${source}`);
      }
      const pe = data.readUInt32LE(0x3c);
      if (pe > data.length - 24 || data.toString("ascii", pe, pe + 4) !== "PE\0\0"
          || data.readUInt16LE(pe + 4) !== machine) {
        throw new Error(`Unexpected PE architecture for ${source}`);
      }
      versions.add(fileVersion(data));
      const relative = `${architecture}/${filename}`;
      const compressed = gzipSync(data, { level: 6 });
      // Normalize the gzip OS byte across Node/zlib platforms.
      compressed[9] = 255;
      files.set(`${relative}.gz`, compressed);
      entries.push({
        path: relative,
        size: data.length,
        sha256: createHash("sha256").update(data).digest("hex"),
      });
    }
  }
  if (versions.size !== 1) throw new Error("ConPTY DLL and hosts have different file versions");
  const manifest = {
    package: PACKAGE,
    version,
    file_version: [...versions][0],
    source: packageUrl(version),
    package_sha256: sha256,
    assets: entries,
  };
  files.set("manifest.json", Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`));
  return { manifest, files };
}

export function writeConptyAssets(prepared, assets = ASSETS) {
  for (const [relative, data] of prepared.files) {
    const destination = path.join(assets, relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, data);
  }
}

async function main() {
  const { values } = parseArgs({
    options: {
      version: { type: "string" },
      sha256: { type: "string" },
      package: { type: "string" },
    },
  });
  if (!values.version || !values.sha256) throw new Error("Supply --version and --sha256");
  validatePin(values.version, values.sha256);
  let data;
  if (values.package) {
    data = fs.readFileSync(values.package);
  } else {
    const response = await fetch(packageUrl(values.version), { signal: AbortSignal.timeout(120000) });
    if (!response.ok) throw new Error(`NuGet download failed: HTTP ${response.status}`);
    data = Buffer.from(await response.arrayBuffer());
  }
  const prepared = prepareConptyAssets(data, values);
  writeConptyAssets(prepared);
  console.log(`Vendored ${PACKAGE} ${values.version} into ${ASSETS}`);
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  main().catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
