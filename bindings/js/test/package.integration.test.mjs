import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

const packageRoot = process.env.SHELL_USE_TEST_PACKAGE_ROOT;

if (packageRoot) {
  test("packed binding works without the CLI", async () => {
    const requireFromPackage = createRequire(join(packageRoot, "package.json"));
    const packageEntry = requireFromPackage.resolve("@microsoft/shell-use");
    assert.equal(
      spawnSync("shell-use", ["--version"], { stdio: "ignore" }).error?.code,
      "ENOENT",
    );

    const { ShellUse } = await import(pathToFileURL(packageEntry).href);
    const session = ShellUse.ephemeral("release-integration");
    try {
      await session.open();
      await session.submit("echo release-integration");
      await session.waitCommand();
      await session.expectText("release-integration", { strict: false });
    } finally {
      await session.closeQuiet();
    }
  });
}
