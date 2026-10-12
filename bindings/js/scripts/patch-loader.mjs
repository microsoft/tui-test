import { readFileSync, writeFileSync } from "node:fs";

// Apply this deterministic extension after each napi-rs loader regeneration.
export function patchLoader(path) {
  let source = readFileSync(path, "utf8");
  if (source.includes("let sharedLibraryPath")) return;
  const original = "const require = createRequire(import.meta.url)";
  if (!source.includes(original)) {
    throw new Error("The generated napi-rs loader changed its require declaration");
  }
  source = source.replace(original, `const nativeRequire = createRequire(import.meta.url)
let sharedLibraryPath
const require = (name) => {
  const binding = nativeRequire(name)
  if (binding && typeof binding.configureLibrary === 'function') {
    const addon = nativeRequire.resolve(name)
    const { basename, dirname, join } = nativeRequire('node:path')
    const tag = basename(addon).replace(/^tui-test\\./, '').replace(/\\.node$/, '')
    const windows = tag.startsWith('win32-')
    const extension = windows ? 'dll' : tag.startsWith('darwin-') ? 'dylib' : 'so'
    sharedLibraryPath = join(dirname(addon), \`\${windows ? '' : 'lib'}tui_test_ffi.\${tag}.\${extension}\`)
    binding.configureLibrary(sharedLibraryPath)
  }
  return binding
}
require.resolve = nativeRequire.resolve`);
  writeFileSync(path, source);
}
