import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

const workflow = fs.readFileSync(new URL("../../workflows/release.yml", import.meta.url), "utf8").replace(/\r\n/g, "\n");
const scripts = new Map(
  [...workflow.matchAll(/^      - name: (.+)\n        shell: bash\n        run: \|\n((?:          .*\n|\n)+)/gm)]
    .map(([, name, body]) => [name, body.replace(/^          /gm, "")]),
);
const branch = "homebrew-update/0.1.0";
const formula = "Formula/tui-test.rb";
const git = (cwd, ...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: "pipe" }).trim();

function fixture(t) {
  const target = path.resolve("target");
  fs.mkdirSync(target, { recursive: true });
  const directory = fs.mkdtempSync(path.join(target, "homebrew-branch-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const remote = path.join(directory, "remote.git");
  git(directory, "init", "--bare", "--initial-branch=main", remote);
  git(directory, "clone", "--quiet", remote, "seed");
  const seed = path.join(directory, "seed");
  git(seed, "config", "user.name", "Release test");
  git(seed, "config", "user.email", "release-test@example.com");
  fs.mkdirSync(path.join(seed, "Formula"));
  fs.writeFileSync(path.join(seed, formula), "old formula\n");
  git(seed, "add", formula);
  git(seed, "commit", "-m", "Initial formula");
  git(seed, "push", "origin", "main");
  return {
    remote,
    remoteGit: (...args) => git(directory, "--git-dir", remote, ...args),
    clone(name) {
      git(directory, "clone", "--quiet", "--branch", "main", remote, name);
      return path.join(directory, name);
    },
  };
}

function runStep(cwd, name) {
  const script = scripts.get(name);
  assert.ok(script, `Missing workflow step: ${name}`);
  return execFileSync("bash", ["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", script], {
    cwd,
    encoding: "utf8",
    stdio: "pipe",
    env: {
      ...process.env,
      RELEASE_TAG: "0.1.0",
      GITHUB_SERVER_URL: "https://github.com",
      GITHUB_REPOSITORY: "fixture/tui-test",
      GITHUB_STEP_SUMMARY: "summary.md",
    },
  });
}

test("Homebrew updates push a branch without changing main and rerun without losing maintainer commits", (t) => {
  const { remoteGit, clone } = fixture(t);
  const first = clone("first");
  runStep(first, "Prepare Homebrew update branch");
  fs.writeFileSync(path.join(first, formula), "released formula\n");
  runStep(first, "Push Homebrew update branch");
  assert.equal(remoteGit("show", `${branch}:${formula}`), "released formula");
  assert.equal(remoteGit("show", `main:${formula}`), "old formula");
  assert.match(remoteGit("log", "-1", "--format=%B", branch), /Signed-off-by: github-actions\[bot\]/);
  const summary = fs.readFileSync(path.join(first, "summary.md"), "utf8");
  assert.ok(summary.includes(`https://github.com/fixture/tui-test/compare/main...${branch}?expand=1`));

  fs.writeFileSync(path.join(first, "maintainer.txt"), "Keep this change\n");
  git(first, "add", "maintainer.txt");
  git(first, "commit", "-m", "Maintainer change");
  git(first, "push", "origin", branch);
  const head = remoteGit("rev-parse", branch);

  const retry = clone("retry");
  runStep(retry, "Prepare Homebrew update branch");
  assert.equal(fs.readFileSync(path.join(retry, "maintainer.txt"), "utf8").replace(/\r\n/g, "\n"), "Keep this change\n");
  runStep(retry, "Push Homebrew update branch");
  assert.equal(remoteGit("rev-parse", branch), head);
  assert.equal(fs.readFileSync(path.join(retry, "summary.md"), "utf8"), summary);

  fs.writeFileSync(path.join(retry, formula), "corrected formula\n");
  runStep(retry, "Push Homebrew update branch");
  assert.equal(remoteGit("show", `${branch}:${formula}`), "corrected formula");
  assert.equal(remoteGit("rev-parse", `${branch}^`), head);
  assert.equal(remoteGit("show", `${branch}:maintainer.txt`), "Keep this change");
});

test("Homebrew branch lookup and push failures do not publish a success summary", (t) => {
  const { remote, remoteGit, clone } = fixture(t);
  const lookup = clone("lookup");
  git(lookup, "remote", "set-url", "origin", `${remote}.missing`);
  assert.throws(() => runStep(lookup, "Prepare Homebrew update branch"), /does not appear to be a git repository/);
  assert.ok(!fs.existsSync(path.join(lookup, "summary.md")));

  const push = clone("push");
  runStep(push, "Prepare Homebrew update branch");
  fs.writeFileSync(path.join(push, formula), "released formula\n");
  git(push, "remote", "set-url", "origin", `${remote}.missing`);
  assert.throws(() => runStep(push, "Push Homebrew update branch"), /does not appear to be a git repository/);
  assert.ok(!fs.existsSync(path.join(push, "summary.md")));
  assert.equal(remoteGit("branch", "--list", branch), "");
});
