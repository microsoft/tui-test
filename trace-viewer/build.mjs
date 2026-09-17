import { build, context } from "esbuild";
import { watchFile, unwatchFile } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { sveltePlugin } from "./svelte-plugin.mjs";

const source = fileURLToPath(new URL(".", import.meta.url));
const destination = fileURLToPath(new URL("../crates/tui-test/assets/trace-viewer/", import.meta.url));
const templatePath = path.join(source, "index.html");
const previewPath = path.join(source, ".preview", "failure.html");
const check = process.argv.includes("--check");
const watch = process.argv.includes("--watch");
const reportArgument = process.argv.indexOf("--report");
const reportPath = reportArgument < 0 ? undefined : process.argv[reportArgument + 1];
if (reportArgument >= 0 && (!reportPath || reportPath.startsWith("--"))) throw new Error("--report requires a saved trace HTML path");
if (check && (watch || reportPath)) throw new Error("--check cannot be combined with --watch or --report");
if (watch && !reportPath) throw new Error("Use npm run dev -- --report <saved trace HTML>");
if (reportPath && path.relative(path.resolve(reportPath), previewPath) === "") {
  throw new Error("Use the original captured report as --report input, not the generated preview");
}

const runtimePackages = ["svelte", "esm-env", "clsx"];
const licenses = await Promise.all(runtimePackages.map(async (name) => {
  const file = name === "svelte" ? "LICENSE.md" : name === "esm-env" ? "LICENSE" : "license";
  const license = (await readFile(path.join(source, "node_modules", name, file), "utf8")).replace(/\r\n/g, "\n").trim();
  return `/*! ${name}\n${license}\n*/`;
}));
const options = {
  absWorkingDir: source, entryPoints: ["src/index.js"], outfile: "report.min.js",
  bundle: true, minify: true, treeShaking: true, charset: "ascii", legalComments: "none",
  target: "es2023", format: "iife", platform: "browser", write: false, metafile: true,
  supported: { "template-literal": false },
  conditions: ["svelte", "browser", "production"], define: { "process.env.NODE_ENV": '"production"' },
  banner: { js: licenses.join("\n") },
  plugins: [sveltePlugin(), {
    name: "standalone-report",
    setup(build) {
      build.onEnd(async (result) => {
        if (result.errors.length) return;
        for (const input of Object.keys(result.metafile.inputs)) {
          const dependency = /node_modules\/([^/]+)/.exec(input.replaceAll("\\", "/"))?.[1];
          if (dependency && !runtimePackages.includes(dependency)) throw new Error(`Unexpected viewer runtime dependency: ${dependency}`);
        }
        const assets = new Map(result.outputFiles.map((file) => [path.basename(file.path), file.text.replace(/\r\n/g, "\n")]));
        for (const name of ["report.min.js", "report.min.css"]) {
          if (!assets.get(name)) throw new Error(`Missing required bundled asset: ${name}`);
        }
        const template = (await readFile(templatePath, "utf8")).replace(/\r\n/g, "\n");
        assets.set("report.html", template);
        const shell = template.replace("/* REPORT_CSS */", () => assets.get("report.min.css"))
          .replace("/* REPORT_JS */", () => assets.get("report.min.js"));
        const bytes = Buffer.byteLength(shell);
        const gzipBytes = gzipSync(shell, { level: 9 }).length;
        if (bytes > 144 * 1024 || gzipBytes > 48 * 1024) {
          throw new Error(`Viewer exceeds the 144 KiB raw / 48 KiB gzip budget: ${bytes} raw / ${gzipBytes} gzip bytes`);
        }
        if (!check) await mkdir(destination, { recursive: true });
        for (const [name, generated] of assets) {
          const output = path.join(destination, name);
          if (check) {
            const existing = await readFile(output, "utf8");
            if (existing.replace(/\r\n/g, "\n") !== generated) {
              throw new Error(`${name} is stale. Run npm run build --prefix trace-viewer.`);
            }
          } else {
            await writeFile(output, generated);
          }
          console.log(`${check ? "Checked" : "Built"} ${name}: ${Buffer.byteLength(generated)} bytes`);
        }
        console.log(`Standalone viewer (excluding trace data): ${bytes} bytes / ${gzipBytes} bytes gzip`);
        if (reportPath) {
          const report = await readFile(reportPath, "utf8");
          const embedded = /<script id="report-data" type="application\/json">([\s\S]*?)<\/script>/.exec(report)?.[1];
          if (!embedded) throw new Error("The supplied HTML has no embedded report-data block");
          const data = JSON.stringify(JSON.parse(embedded)).replace(/[<>&\u2028\u2029]/g,
            (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`);
          await mkdir(path.dirname(previewPath), { recursive: true });
          await writeFile(previewPath, shell.replace("null /* REPORT_DATA */", () => data));
          console.log(`Preview: ${previewPath}${watch ? " (reload your browser after edits)" : ""}`);
        }
      });
    },
  }],
};

if (watch) {
  const watcher = await context(options);
  await watcher.watch();
  const watchedFiles = [templatePath, path.resolve(reportPath)];
  for (const file of watchedFiles) watchFile(file, { interval: 500 }, () => {
    watcher.rebuild().catch((error) => console.error(error.message));
  });
  process.once("SIGINT", async () => {
    for (const file of watchedFiles) unwatchFile(file);
    await watcher.dispose();
  });
} else {
  await build(options);
}
