// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import path from "node:path";
import process from "node:process";
import module from "node:module";
import url from "node:url";

const stripExt = (name: string) => {
  const extension = path.extname(name);
  return !extension ? name : name.slice(0, -extension.length);
};

export const isMain = (meta: ImportMeta) => {
  if (!meta || !process.argv[1]) {
    return false;
  }

  const require = module.createRequire(meta.url);
  const scriptPath = require.resolve(process.argv[1]);

  const modulePath = url.fileURLToPath(meta.url);

  const extension = path.extname(scriptPath);
  if (extension) {
    return modulePath === scriptPath;
  }

  return stripExt(modulePath) === scriptPath;
};
