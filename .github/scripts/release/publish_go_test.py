"""Exercise the workflow's Git commands against a temporary local remote."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch

import package_native_test as fixtures


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.PackageTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.remote = self.root / "remote.git"
        workflow = (fixtures.ROOT / ".github/workflows/release.yml").read_text()
        step = workflow.split("      - name: Publish immutable Go module tag\n", 1)[1]
        self.script = textwrap.dedent(step.split("        run: |\n", 1)[1].split("\n  release:", 1)[0])

        script = self.root / ".github/scripts/release/package_native.py"
        script.parent.mkdir(parents=True)
        shutil.copy2(fixtures.SCRIPTS / "package_native.py", script)
        self.git("init", "-q", "-b", "main")
        # Use the workflow's existing automation identity only in this fixture.
        identity = "\n".join(line for line in self.script.splitlines() if line.startswith("git config user."))
        subprocess.run(["bash", "-ec", identity], cwd=self.root, check=True)
        self.git("config", "commit.gpgsign", "false")
        self.git("add", "bindings", "LICENSE", ".github")
        self.git("commit", "-q", "-s", "-m", "Source release fixture")
        self.revision = self.git("rev-parse", "HEAD")
        self.git("tag", "1.2.3")
        self.git("tag", "1.2.4")
        self.git("init", "-q", "--bare", "-b", "main", str(self.remote))
        self.git("remote", "add", "origin", str(self.remote))
        self.git("push", "-q", "origin", "main", "--tags")
        with patch.object(fixtures, "REVISION", self.revision):
            for target in fixtures.TARGETS:
                self.fixture.write_native(target)
        self.fixture.run_script(
            "go", "--artifacts", self.fixture.artifacts, "--output", self.fixture.output,
        )

    def git(self, *args, cwd=None):
        command = ["git"]
        if cwd == self.remote:
            command += ["--git-dir", str(self.remote)]
        return subprocess.check_output(
            [*command, *args], cwd=cwd or self.root, text=True, stderr=subprocess.PIPE,
        ).strip()

    def publish(self, version="1.2.3"):
        workspace = Path(tempfile.mkdtemp(prefix="publisher-", dir=self.root))
        self.git("clone", "-q", "--no-checkout", str(self.remote), str(workspace))
        self.git("checkout", "-q", "--detach", version, cwd=workspace)
        self.git("config", "commit.gpgsign", "false", cwd=workspace)
        shutil.copytree(self.fixture.output, workspace / "target/go-package")
        return subprocess.run(
            ["bash", "-c", self.script], cwd=workspace,
            env={**os.environ, "RELEASE_TAG": version},
            capture_output=True, text=True,
        )

    def remote_ref(self, ref):
        return self.git("rev-parse", ref, cwd=self.remote)

    def test_release_only_tag_and_identical_retry(self):
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        tag = "refs/tags/bindings/go/v1.2.3"
        commit = self.remote_ref(tag)
        self.assertEqual(self.remote_ref(tag + "^"), self.revision)
        self.assertEqual(self.remote_ref("refs/heads/main"), self.revision)
        self.assertEqual(self.remote_ref("refs/tags/1.2.3"), self.revision)
        files = self.git("ls-tree", "-r", "--name-only", tag, cwd=self.remote).splitlines()
        self.assertEqual(len([name for name in files if name.endswith(".tar.gz")]), 7)
        module = fixtures.packages.directory_entries(self.fixture.output)
        self.assertEqual(
            set(self.git("ls-tree", "-r", "--name-only", f"{tag}:bindings/go", cwd=self.remote).splitlines()),
            set(module),
        )
        for name, expected in module.items():
            actual = subprocess.check_output([
                "git", "--git-dir", str(self.remote), "show", f"{tag}:bindings/go/{name}",
            ])
            self.assertEqual(actual, expected, name)
        self.assertIn("Signed-off-by:", self.git("show", "-s", "--format=%B", tag, cwd=self.remote))
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("already contains the expected Go module", result.stdout)
        self.assertEqual(self.remote_ref(tag), commit)

    def test_existing_tag_with_different_content_or_parent_is_immutable(self):
        go_mod = self.fixture.source / "go.mod"
        go_mod.write_text(go_mod.read_text() + "// Conflicting release fixture\n")
        self.git("add", "bindings/go/go.mod")
        self.git("commit", "-q", "-s", "-m", "Different release fixture")
        conflicting = self.git("rev-parse", "HEAD")
        tag = "refs/tags/bindings/go/v1.2.3"
        self.git("push", "-q", "origin", f"HEAD:{tag}")
        result = self.publish()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to replace", result.stderr)
        self.assertEqual(self.remote_ref(tag), conflicting)
        self.git("update-ref", tag, self.revision, cwd=self.remote)
        result = self.publish()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to replace", result.stderr)
        self.assertEqual(self.remote_ref(tag), self.revision)

    def test_wrong_version_and_changed_package_are_rejected(self):
        result = self.publish("1.2.4")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Release tag does not match", result.stderr)
        (self.fixture.output / "go.mod").write_text("module changed\n")
        result = self.publish()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match the source revision", result.stderr)
        refs = self.git("for-each-ref", "--format=%(refname)", "refs/tags/bindings/go", cwd=self.remote)
        self.assertEqual(refs, "")
