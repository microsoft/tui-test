import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import { readArchive, sha256 } from "./release/package-native.mjs";

const { values: args } = parseArgs({ options: { archive: { type: "string" } } });
if (!args.archive) throw new Error("Usage: test-c-sdk.mjs --archive PATH");
fs.mkdirSync("target", { recursive: true });
const scratch = fs.mkdtempSync(path.resolve("target/c-sdk-test-"));
try {
  const original = path.join(scratch, "original");
  fs.mkdirSync(original);
  const members = await readArchive(fs.readFileSync(args.archive));
  const names = new Set();
  const manifest = JSON.parse(members.find(member => member.name === "manifest.json")?.data ?? "null");
  if (!manifest || !manifest.files) throw new Error("Missing SDK manifest");
  for (const member of members) {
    if (!["File", "OldFile"].includes(member.type)
        || !/^[a-zA-Z0-9_.+-]+(?:\/[a-zA-Z0-9_.+-]+)*$/.test(member.name)
        || member.name.split("/").some(part => part === "." || part === "..")
        || names.has(member.name)) throw new Error(`Unexpected SDK member: ${member.name}`);
    names.add(member.name);
    if (member.name !== "manifest.json" && manifest.files[member.name] !== sha256(member.data)) {
      throw new Error(`SDK checksum mismatch: ${member.name}`);
    }
    const destination = path.join(original, member.name);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, member.data, { mode: member.mode });
  }
  if (Object.keys(manifest.files).length !== names.size - 1) throw new Error("SDK files are missing");
  const sdk = path.join(scratch, "relocated SDK");
  fs.renameSync(original, sdk);
  const header = fs.readFileSync(path.join(sdk, "include/tui_test.h"), "utf8");
  const symbols = [...new Set([...header.matchAll(/\b(tui_[a-z0-9_]+)\s*\(/g)].map(match => match[1]))];
  if (!symbols.length) throw new Error("The header declares no C API functions");
  fs.writeFileSync(path.join(sdk, "examples/exports.c"),
    '#include "tui_test.h"\nint main(void) {\n  volatile uintptr_t symbol = 0;\n'
    + symbols.map(name => `  symbol ^= (uintptr_t)&${name};\n`).join("")
    + "  (void)symbol;\n  return tui_abi_version() != ABI_VERSION;\n}\n");
  fs.appendFileSync(path.join(sdk, "examples/CMakeLists.txt"),
    "\nadd_executable(exports exports.c)\ntarget_link_libraries(exports PRIVATE tui-test::ffi)\n"
    + "add_test(NAME exports COMMAND exports)\n");
  const build = path.join(scratch, "consumer");
  const run = (command, argv) => execFileSync(command, argv, { stdio: "inherit" });
  run("cmake", ["-S", path.join(sdk, "examples"), "-B", build,
    `-DCMAKE_PREFIX_PATH=${sdk}`, "-DCMAKE_BUILD_TYPE=Release"]);
  run("cmake", ["--build", build, "--config", "Release"]);
  run("ctest", ["--test-dir", build, "-C", "Release", "--verbose"]);
  const cli = path.join(sdk, "bin", process.platform === "win32" ? "tui-test.exe" : "tui-test");
  if (fs.existsSync(cli)) run(cli, ["--version"]);
  const installed = path.join(scratch, "installed");
  run("cmake", ["--install", build, "--config", "Release", "--prefix", installed]);
  const application = path.join(scratch, "relocated application");
  fs.renameSync(installed, application);
  fs.rmSync(sdk, { recursive: true, force: true });
  for (const example of ["basic", "async", "cpp"]) {
    const executable = path.join(application, "bin", example + (process.platform === "win32" ? ".exe" : ""));
    const output = execFileSync(executable, [], { encoding: "utf8", timeout: 30000 });
    process.stdout.write(output);
    if (!output.includes("Captured: Hello from C SDK") || !output.includes("Closed session.")) {
      throw new Error(`Incomplete output from ${example}`);
    }
  }
} finally {
  fs.rmSync(scratch, { recursive: true, force: true });
}
