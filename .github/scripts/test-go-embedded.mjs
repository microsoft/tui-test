import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const { positionals } = parseArgs({ allowPositionals: true });
if (positionals.length !== 1) throw new Error("Usage: test-go-embedded.mjs <package>");
const packagePath = path.resolve(positionals[0]);
const root = fileURLToPath(new URL("../../", import.meta.url));
const target = path.join(root, "target");
fs.mkdirSync(target, { recursive: true });
const directory = fs.mkdtempSync(path.join(target, "go-consumer-"));
try {
  const goroot = execFileSync("go", ["env", "GOROOT"], { encoding: "utf8" }).trim();
  const go = path.join(goroot, "bin", process.platform === "win32" ? "go.exe" : "go");
  const environment = { ...process.env, CGO_ENABLED: "0", GOWORK: "off", GOTOOLCHAIN: "local", GOROOT: goroot };
  // Windows environment names are case-insensitive, including Path/PATH.
  for (const name of Object.keys(environment)) {
    if (["PATH", "TUI_TEST_NATIVE_LIBRARY", "GOFLAGS", "GOOS", "GOARCH"].includes(name.toUpperCase())) delete environment[name];
  }
  environment.PATH = "";
  for (const args of [
    ["mod", "init", "example.com/tui-test-consumer"],
    ["mod", "edit", `-replace=github.com/microsoft/tui-test/bindings/go=${packagePath}`],
    ["get", "github.com/microsoft/tui-test/bindings/go@v0.0.0"],
    ["build", "-buildvcs=false", "-o", path.join(directory, "smoke.exe"), "github.com/microsoft/tui-test/bindings/go/examples/smoke"],
  ]) {
    execFileSync(go, args, { cwd: directory, env: environment, stdio: "inherit" });
  }
  const runtime = {
    ...environment, HOME: path.join(directory, "home"),
    XDG_CACHE_HOME: path.join(directory, "cache"), LOCALAPPDATA: path.join(directory, "cache"),
  };
  for (const attempt of ["first extraction", "cache reuse"]) {
    const output = execFileSync(path.join(directory, "smoke.exe"), [], {
      cwd: directory, env: runtime, encoding: "utf8", timeout: 60000,
    });
    if (!output.includes("Go binding smoke passed (bundled engine)")) throw new Error(`Unexpected consumer output: ${output}`);
    console.log(`${attempt}: ${output.trim()}`);
  }
  const readme = fs.readFileSync(path.join(packagePath, "README.md"), "utf8");
  const example = readme.split("```go\n")[1]?.split("\n```")[0];
  if (!example) throw new Error("README Go example is missing");
  fs.writeFileSync(path.join(directory, "main.go"), `${example}\n`);
  execFileSync(go, ["build", "-buildvcs=false", "-o", path.join(directory, "readme.exe"), "."], {
    cwd: directory, env: environment, stdio: "inherit",
  });
  const hostPath = Object.entries(process.env).find(([name]) => name.toUpperCase() === "PATH")?.[1] ?? "";
  const output = execFileSync(path.join(directory, "readme.exe"), [], {
    cwd: directory, env: { ...runtime, PATH: hostPath }, encoding: "utf8", timeout: 60000,
  });
  if (!output.includes("hello")) throw new Error(`README example failed: ${output}`);
  console.log(`README quick start: ${output.trim()}`);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
