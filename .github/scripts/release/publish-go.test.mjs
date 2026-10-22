import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { createFixture, root, scripts, targets } from "./release-fixtures.mjs";
import { directoryEntries } from "./package-native.mjs";

function publicationFixture(t) {
  const fixture = createFixture(t);
  const remote = path.join(fixture.root, "remote.git");
  const workflow = fs.readFileSync(path.join(root, ".github/workflows/release.yml"), "utf8");
  const step = workflow.split("      - name: Publish immutable Go module tag\n")[1];
  const script = step.split("        run: |\n")[1].split("\n  release:")[0].replace(/^ {10}/gm, "");
  const filename = path.join(fixture.root, ".github/scripts/release/package-native.mjs");
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.copyFileSync(path.join(scripts, "package-native.mjs"), filename);
  function git(args, cwd = fixture.root, encoding = "utf8") {
    return execFileSync("git", [...(cwd === remote ? ["--git-dir", remote] : []), ...args], {
      cwd, encoding, stdio: ["ignore", "pipe", "pipe"],
    });
  }
  git(["init", "-q", "-b", "main"]);
  const identity = script.split("\n").filter(line => line.startsWith("git config user.")).join("\n");
  assert.ok(identity, "workflow automation identity is missing");
  execFileSync("bash", ["-ec", identity], { cwd: fixture.root });
  git(["config", "commit.gpgsign", "false"]);
  git(["add", "bindings", "LICENSE", ".github"]);
  git(["commit", "-q", "-s", "-m", "Source release fixture"]);
  fixture.revision = git(["rev-parse", "HEAD"]).trim();
  git(["tag", "1.2.3"]);
  git(["tag", "1.2.4"]);
  git(["init", "-q", "--bare", "-b", "main", remote]);
  git(["remote", "add", "origin", remote]);
  git(["push", "-q", "origin", "main", "--tags"]);
  for (const target of Object.keys(targets)) fixture.writeNative(target);
  fixture.assemble();
  return {
    fixture, remote, git,
    ref(name) { return git(["rev-parse", name], remote).trim(); },
    publish(version = "1.2.3") {
      const workspace = fs.mkdtempSync(path.join(fixture.root, "publisher-"));
      git(["clone", "-q", "--no-checkout", remote, workspace]);
      git(["checkout", "-q", "--detach", version], workspace);
      git(["config", "commit.gpgsign", "false"], workspace);
      fs.cpSync(fixture.output, path.join(workspace, "target/go-package"), { recursive: true });
      fs.symlinkSync(path.join(scripts, "../node_modules"), path.join(workspace, "node_modules"), "junction");
      const result = spawnSync("bash", ["-c", script], {
        cwd: workspace, env: { ...process.env, RELEASE_TAG: version }, encoding: "utf8",
      });
      if (result.error) throw result.error;
      return result;
    },
  };
}

test("publication creates a release-only tag and accepts identical retries", t => {
  const p = publicationFixture(t);
  let result = p.publish();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const tag = "refs/tags/bindings/go/v1.2.3";
  const commit = p.ref(tag);
  assert.equal(p.ref(`${tag}^`), p.fixture.revision);
  assert.equal(p.ref("refs/heads/main"), p.fixture.revision);
  assert.equal(p.ref("refs/tags/1.2.3"), p.fixture.revision);
  const files = p.git(["ls-tree", "-r", "--name-only", tag], p.remote).trim().split("\n");
  assert.equal(files.filter(name => name.endsWith(".tar.gz")).length, 7);
  const module = directoryEntries(p.fixture.output);
  const names = p.git(["ls-tree", "-r", "--name-only", `${tag}:bindings/go`], p.remote).trim().split("\n");
  assert.deepEqual(new Set(names), new Set(module.keys()));
  for (const [name, expected] of module) {
    assert.deepEqual(p.git(["show", `${tag}:bindings/go/${name}`], p.remote, null), expected, name);
  }
  assert.match(p.git(["show", "-s", "--format=%B", tag], p.remote), /Signed-off-by:/);
  result = p.publish();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /already contains the expected Go module/);
  assert.equal(p.ref(tag), commit);
});

test("existing tags with different content or parent remain immutable", t => {
  const p = publicationFixture(t);
  fs.appendFileSync(path.join(p.fixture.source, "go.mod"), "// Conflicting release fixture\n");
  p.git(["add", "bindings/go/go.mod"]);
  p.git(["commit", "-q", "-s", "-m", "Different release fixture"]);
  const conflicting = p.git(["rev-parse", "HEAD"]).trim();
  const tag = "refs/tags/bindings/go/v1.2.3";
  p.git(["push", "-q", "origin", `HEAD:${tag}`]);
  let result = p.publish();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /refusing to replace/);
  assert.equal(p.ref(tag), conflicting);
  p.git(["update-ref", tag, p.fixture.revision], p.remote);
  result = p.publish();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /refusing to replace/);
  assert.equal(p.ref(tag), p.fixture.revision);
});

test("wrong release versions and changed modules cannot publish tags", t => {
  const p = publicationFixture(t);
  let result = p.publish("1.2.4");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Release tag does not match/);
  fs.writeFileSync(path.join(p.fixture.output, "go.mod"), "module changed\n");
  result = p.publish();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /does not match the source revision/);
  assert.equal(p.git(["for-each-ref", "--format=%(refname)", "refs/tags/bindings/go"], p.remote).trim(), "");
});
