import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const GO_VERSION_FILES = [
  "README.md",
  "bindings/go/README.md",
  "docs/website/install.mdx",
  "docs/website/api/go.mdx",
];

export function verifyGoVersion(root, version) {
  for (const name of GO_VERSION_FILES) {
    const filename = path.join(root, name);
    if (!fs.existsSync(filename) && name.startsWith("docs/website/")) continue;
    const text = fs.readFileSync(filename, "utf8");
    const commands = [...text.matchAll(/go get github\.com\/microsoft\/tui-test\/bindings\/go@([^\s`]+)/g)];
    if (!commands.length || commands.some(match => match[1] !== `v${version}`)) {
      throw new Error(`${name} must use go get github.com/microsoft/tui-test/bindings/go@v${version}`);
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const version = process.argv[2] ?? JSON.parse(fs.readFileSync("bindings/js/package.json", "utf8")).version;
  verifyGoVersion(process.cwd(), version);
  console.log(`Go installation examples match v${version}`);
}
