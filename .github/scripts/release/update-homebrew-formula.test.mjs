import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

import {
  HOMEBREW_ASSETS,
  updateHomebrewFormula,
} from "./update-homebrew-formula.mjs";

const OLD_VERSION = "0.1.0-beta.2";
const NEW_VERSION = "0.1.0-beta.3";
const OLD_HASH = "0".repeat(64);

function renderFormula(version, hashes) {
  const blocks = HOMEBREW_ASSETS.map(
    (asset) =>
      `  url "https://github.com/microsoft/tui-test/releases/download/${version}/${asset}"\n` +
      `  sha256 "${hashes.get(asset)}"`,
  ).join("\n");

  return `class TuiTest < Formula
  version "${version}"
${blocks}
end
`;
}

function createFixture({ missingAsset } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tui-test-homebrew-"));
  const artifacts = path.join(root, "artifacts");
  const formula = path.join(root, "tui-test.rb");
  const hashes = new Map();
  fs.mkdirSync(artifacts);

  for (const asset of HOMEBREW_ASSETS) {
    const contents = `contents of ${asset}\n`;
    hashes.set(
      asset,
      createHash("sha256").update(contents).digest("hex"),
    );
    if (asset !== missingAsset) {
      fs.writeFileSync(path.join(artifacts, asset), contents);
    }
  }

  fs.writeFileSync(
    formula,
    renderFormula(
      OLD_VERSION,
      new Map(HOMEBREW_ASSETS.map((asset) => [asset, OLD_HASH])),
    ),
  );

  return { artifacts, formula, hashes, root };
}

test("updates every release URL and checksum", () => {
  const fixture = createFixture();
  try {
    const result = updateHomebrewFormula(
      NEW_VERSION,
      fixture.artifacts,
      fixture.formula,
    );

    assert.equal(result.changed, true);
    assert.deepEqual(result.hashes, fixture.hashes);
    assert.equal(
      fs.readFileSync(fixture.formula, "utf8"),
      renderFormula(NEW_VERSION, fixture.hashes),
    );

    const secondResult = updateHomebrewFormula(
      NEW_VERSION,
      fixture.artifacts,
      fixture.formula,
    );
    assert.equal(secondResult.changed, false);
  } finally {
    fs.rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("rejects a missing release asset", () => {
  const missingAsset = HOMEBREW_ASSETS.at(-1);
  const fixture = createFixture({ missingAsset });
  try {
    assert.throws(
      () =>
        updateHomebrewFormula(
          NEW_VERSION,
          fixture.artifacts,
          fixture.formula,
        ),
      new RegExp(`Missing release asset: .*${missingAsset}`),
    );
  } finally {
    fs.rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("rejects a formula with an incomplete platform list", () => {
  const fixture = createFixture();
  try {
    const incompleteFormula = fs
      .readFileSync(fixture.formula, "utf8")
      .replace(
        /^.*tui-test-x86_64-unknown-linux-gnu\.tar\.gz.*\r?\n.*\r?\n/m,
        "",
      );
    fs.writeFileSync(fixture.formula, incompleteFormula);

    assert.throws(
      () =>
        updateHomebrewFormula(
          NEW_VERSION,
          fixture.artifacts,
          fixture.formula,
        ),
      /Formula release assets must be exactly:/,
    );
  } finally {
    fs.rmSync(fixture.root, { recursive: true, force: true });
  }
});
