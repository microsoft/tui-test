import { build } from "esbuild";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const directory = fileURLToPath(new URL("../../../crates/tui-test/src/diagnostics/", import.meta.url));
const check = process.argv.includes("--check");
for (const [entry, output] of [["viewer/index.ts", "report.min.js"], ["report.css", "report.min.css"]]) {
  const result = await build({
    absWorkingDir: directory,
    entryPoints: [entry],
    outfile: output,
    bundle: true,
    minify: true,
    write: false,
    charset: "ascii",
    legalComments: "none",
    target: "es2023",
    format: "iife",
    platform: "browser",
  });
  const generated = result.outputFiles[0].text;
  const destination = path.join(directory, output);
  if (check) {
    const existing = await readFile(destination, "utf8");
    if (existing.replace(/\r\n/g, "\n") !== generated) {
      throw new Error(`${output} is stale. Run npm run build:report in bindings/js.`);
    }
  } else {
    await writeFile(destination, generated);
  }
  console.log(`${check ? "Checked" : "Built"} ${output}: ${Buffer.byteLength(generated)} bytes`);
}
