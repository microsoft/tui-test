import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { createFixture } from "./release-fixtures.mjs";
import { GO_VERSION_FILES, verifyGoVersion } from "./verify-go-version.mjs";

test("release checks require every Go installation example to match the tag", t => {
  const fixture = createFixture(t);
  const current = "go get github.com/microsoft/tui-test/bindings/go@v1.2.3";
  for (const name of GO_VERSION_FILES) {
    const filename = path.join(fixture.root, name);
    fs.mkdirSync(path.dirname(filename), { recursive: true });
    fs.writeFileSync(filename, current);
  }
  verifyGoVersion(fixture.root, "1.2.3");
  for (const name of GO_VERSION_FILES) {
    const filename = path.join(fixture.root, name);
    for (const text of ["", "go get github.com/microsoft/tui-test/bindings/go@v<version>",
      current.replace("1.2.3", "1.2.2"), `${current}\n${current.replace("1.2.3", "1.2.2")}`]) {
      fs.writeFileSync(filename, text);
      assert.throws(() => verifyGoVersion(fixture.root, "1.2.3"), /must use go get/);
    }
    fs.writeFileSync(filename, current);
  }
  assert.throws(() => verifyGoVersion(fixture.root, "1.2.4"), /must use go get/);
});
