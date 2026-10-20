import gzip
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import package_native as packages


SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[2]
TARGETS = json.loads((ROOT / "bindings/js/package.json").read_text())["nativeTargets"]
REVISION = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()


def load_script(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def library_fixture(target):
    data = bytearray(128)
    arm = target.startswith("aarch64")
    if "linux" in target:
        data[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<HH", data, 16, 3, 183 if arm else 62)
    elif "apple" in target:
        struct.pack_into("<IIII", data, 0, 0xFEEDFACF, 0x100000C if arm else 0x1000007, 0, 6)
    else:
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<H", data, 68, 0x8664)
        struct.pack_into("<H", data, 86, 0x2000)
    return bytes(data)


class PackageTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "target").mkdir(exist_ok=True)
        scratch = tempfile.TemporaryDirectory(prefix="release-test-", dir=ROOT / "target")
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        (self.root / "bindings/js").mkdir(parents=True)
        (self.root / "bindings/js/package.json").write_text(json.dumps({
            "version": "1.2.3", "nativeTargets": TARGETS,
        }))
        (self.root / "LICENSE").write_text("License fixture\n")
        self.source = self.root / "bindings/go"
        (self.source / "internal/native").mkdir(parents=True)
        (self.source / "go.mod").write_text("module example.com/fixture\n\ngo 1.23\n")
        (self.source / "internal/native/registry.go").write_text(
            "package native\n"
            "type bundledLibrary struct { name string; checksum string; archive []byte }\n"
            "var bundledLibraries = map[string]bundledLibrary{}\n"
        )
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        self.output = self.root / "assembled"
        for target in TARGETS:
            self.write_native(target)

    def write_native(self, target):
        data = library_fixture(target)
        manifest = {
            "version": "1.2.3", "revision": REVISION, "target": target,
            "library": packages.library_name(target), "sha256": packages.sha256(data),
        }
        path = self.artifacts / packages.native_filename("1.2.3", target)
        packages.write_archive(path, {
            manifest["library"]: data, "LICENSE": b"License fixture\n",
            "manifest.json": json.dumps(manifest).encode(),
        })
        return path

    def run_script(self, command, *args, succeeds=True):
        result = subprocess.run(
            [sys.executable, str(SCRIPTS / "package_native.py"), command, *map(str, args)],
            cwd=self.root, capture_output=True, text=True,
        )
        if succeeds:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def test_native_cli_deterministic_all_platforms(self):
        for target in TARGETS:
            with self.subTest(target=target):
                library = self.root / packages.library_name(target)
                library.write_bytes(library_fixture(target))
                args = ("--library", library, "--target", target)
                self.run_script("native", *args)
                path = self.root / "target/native-packages" / packages.native_filename("1.2.3", target)
                first = path.read_bytes()
                self.run_script("native", *args)
                self.assertEqual(path.read_bytes(), first)
                self.assertEqual(first[4:8], b"\0\0\0\0")
                self.assertEqual(path.with_name(path.name + ".sha256").read_text(),
                                 f"{packages.sha256(first)}  {path.name}\n")
                self.assertEqual(packages.read_native(first, "1.2.3", REVISION, target)["target"], target)
                with tarfile.open(path) as archive:
                    self.assertEqual(len(archive.getmembers()), 3)
                    for member in archive.getmembers():
                        self.assertEqual((member.uid, member.gid, member.mtime, member.mode), (0, 0, 0, 0o644))

    def test_wrong_platform_rejected(self):
        library = self.root / "wrong.so"
        library.write_bytes(library_fixture("aarch64-apple-darwin"))
        self.run_script("native", "--library", library,
                        "--target", "aarch64-unknown-linux-gnu", succeeds=False)
        self.assertFalse((self.root / "target/native-packages").exists())

    def test_module_all_targets_and_source_unchanged(self):
        before = packages.directory_entries(self.source)
        self.run_script("go", "--artifacts", self.artifacts, "--output", self.output)
        self.assertEqual(packages.directory_entries(self.source), before)
        self.assertEqual(packages.verify_module(self.root, self.output),
                         packages.directory_entries(self.output))
        self.assertEqual((self.output / "LICENSE").read_bytes(), (self.root / "LICENSE").read_bytes())
        self.assertEqual(len(list((self.output / "internal/native").glob("bundled_*.go"))), 5)
        self.assertEqual(len(list((self.output / "internal/native/bundled").glob("*.tar.gz"))), 7)
        self.assertTrue((self.root / "tui-test-go-1.2.3.tar.gz.sha256").is_file())
        archive = self.root / "tui-test-go-1.2.3.tar.gz"
        with tarfile.open(archive) as packed:
            self.assertEqual({entry.name for entry in packed.getmembers()},
                             set(packages.directory_entries(self.output)))
        self.run_script("go", "--artifacts", self.artifacts, "--output", self.output, succeeds=False)

    def test_platform_selection_and_build(self):
        self.run_script("go", "--artifacts", self.artifacts, "--output", self.output)
        scratch = self.root / "go-tmp"
        scratch.mkdir()
        for platform, arch in (("linux", "amd64"), ("linux", "arm64"), ("darwin", "amd64"),
                               ("darwin", "arm64"), ("windows", "amd64")):
            with self.subTest(platform=platform, arch=arch):
                env = {
                    **os.environ, "GOOS": platform, "GOARCH": arch, "CGO_ENABLED": "0",
                    "GOWORK": "off", "GOTMPDIR": str(scratch), "TMPDIR": str(scratch),
                    "GOCACHE": str(scratch / "cache"),
                }
                result = subprocess.check_output(
                    ["go", "list", "-json", "./internal/native"], cwd=self.output, env=env, text=True,
                )
                details = json.loads(result)
                self.assertIn(f"bundled_{platform}_{arch}.go", details["GoFiles"])
                self.assertEqual(len(details["EmbedFiles"]), 2 if platform == "linux" else 1)
                subprocess.run(["go", "build", "./..."], cwd=self.output, env=env, check=True,
                               capture_output=True)

    def test_partial_target_selection(self):
        target = "aarch64-apple-darwin"
        self.run_script("go", "--artifacts", self.artifacts, "--target", target)
        output = self.root / "target/go-package"
        self.assertEqual(len(list((output / "internal/native/bundled").iterdir())), 1)
        self.assertTrue((output / "internal/native/bundled_darwin_arm64.go").is_file())

    def test_missing_extra_duplicate_and_unknown_targets(self):
        path = self.write_native("aarch64-apple-darwin")
        saved = path.read_bytes()
        path.unlink()
        self.run_script("go", "--artifacts", self.artifacts, "--output", self.output, succeeds=False)
        self.assertFalse(self.output.exists())
        path.write_bytes(saved)
        extra = self.artifacts / "extra.tar.gz"
        extra.write_bytes(saved)
        self.run_script("go", "--artifacts", self.artifacts, "--output", self.output, succeeds=False)
        extra.unlink()
        for targets in (["unsupported"], ["aarch64-apple-darwin"] * 2):
            args = [arg for target in targets for arg in ("--target", target)]
            self.run_script("go", "--artifacts", self.artifacts, "--output", self.output,
                            *args, succeeds=False)
        self.assertFalse(self.output.exists())

    def test_tampered_manifest_library_and_archive_checksum(self):
        target = "aarch64-unknown-linux-gnu"
        path = self.write_native(target)
        with tarfile.open(path) as archive:
            original = {member.name: archive.extractfile(member).read() for member in archive.getmembers()}
        for field in ("version", "revision", "target", "library", "sha256", "extra"):
            with self.subTest(field=field):
                contents = dict(original)
                manifest = json.loads(contents["manifest.json"])
                manifest[field] = "tampered"
                contents["manifest.json"] = json.dumps(manifest).encode()
                packages.write_archive(path, contents)
                self.run_script("go", "--artifacts", self.artifacts,
                                "--output", self.output, succeeds=False)
                self.assertFalse(self.output.exists())
        contents = dict(original)
        contents[packages.library_name(target)] += b"tampered"
        packages.write_archive(path, contents)
        self.run_script("go", "--artifacts", self.artifacts,
                        "--output", self.output, succeeds=False)
        self.write_native(target)
        path.with_name(path.name + ".sha256").write_text("0" * 64)
        self.run_script("go", "--artifacts", self.artifacts,
                        "--output", self.output, succeeds=False)

    def test_unsafe_archive_members(self):
        target = "x86_64-unknown-linux-musl"
        path = self.write_native(target)
        for kind in ("traversal", "symlink", "hardlink", "duplicate", "extra", "absolute"):
            with self.subTest(kind=kind):
                original = self.write_native(target).read_bytes()
                stream = io.BytesIO()
                with tarfile.open(fileobj=io.BytesIO(original)) as source:
                    with tarfile.open(fileobj=stream, mode="w:gz") as dest:
                        for member in source.getmembers():
                            data = source.extractfile(member).read()
                            if member.name == packages.library_name(target):
                                if kind == "traversal":
                                    member.name = "../outside"
                                elif kind == "absolute":
                                    member.name = "/outside"
                                elif kind in ("symlink", "hardlink"):
                                    member.type = tarfile.SYMTYPE if kind == "symlink" else tarfile.LNKTYPE
                                    member.linkname = "../outside"
                                elif kind == "duplicate":
                                    dest.addfile(member, io.BytesIO(data))
                            dest.addfile(member, io.BytesIO(data))
                        if kind == "extra":
                            dest.addfile(tarfile.TarInfo("extra"))
                path.write_bytes(stream.getvalue())
                path.with_name(path.name + ".sha256").unlink()
                self.run_script("go", "--artifacts", self.artifacts,
                                "--output", self.output, succeeds=False)
                self.assertFalse(self.output.exists())

    def test_invalid_gzip_trailers_rejected(self):
        target = "aarch64-unknown-linux-gnu"
        path = self.write_native(target)
        original = path.read_bytes()
        self.run_script("go", "--artifacts", self.artifacts, "--output", self.output)
        damaged = {
            "truncated": original[:-1],
            "crc": original[:-8] + bytes([original[-8] ^ 1]) + original[-7:],
            "size": original[:-4] + bytes([original[-4] ^ 1]) + original[-3:],
        }
        for damage, data in damaged.items():
            with self.subTest(damage=damage, stage="reader"):
                with self.assertRaises((EOFError, gzip.BadGzipFile)):
                    packages.read_native(data, "1.2.3", REVISION, target)
            with self.subTest(damage=damage, stage="assembly"):
                path.write_bytes(data)
                path.with_name(path.name + ".sha256").write_text(
                    f"{packages.sha256(data)}  {path.name}\n",
                )
                output = self.root / f"invalid-module-{damage}"
                self.run_script("go", "--artifacts", self.artifacts,
                                "--output", output, succeeds=False)
                self.assertFalse(output.exists())
            with self.subTest(damage=damage, stage="verification"):
                (self.output / f"internal/native/bundled/{target}.tar.gz").write_bytes(data)
                with self.assertRaises((EOFError, gzip.BadGzipFile)):
                    packages.verify_module(self.root, self.output)

    def test_size_limit_and_source_output_rejected(self):
        archives = packages.native_archives(self.root, self.artifacts)
        with patch.object(packages, "MAX_MODULE_SIZE", 1):
            with self.assertRaisesRegex(ValueError, "500 MiB"):
                packages.module_entries(self.root, archives)
        self.run_script("go", "--artifacts", self.artifacts,
                        "--output", self.source / "new-directory", succeeds=False)
        self.assertFalse((self.source / "new-directory").exists())


class ElfChecks(unittest.TestCase):
    def test_unversioned_required_symbols_rejected(self):
        elf = load_script("check-native-elf")
        elf.check_symbols("\n".join([
            "0: 0000000000000000 0 NOTYPE LOCAL DEFAULT UND",
            "1: 0000000000000000 0 FUNC GLOBAL DEFAULT UND memcpy@GLIBC_2.14 (2)",
            "2: 0000000000000000 0 NOTYPE WEAK DEFAULT UND _ITM_registerTMCloneTable",
            "3: 0000000000000010 8 FUNC GLOBAL DEFAULT 12 tui_open",
        ]))
        for kind in ("FUNC", "NOTYPE"):
            with self.subTest(kind=kind):
                with self.assertRaisesRegex(ValueError, "arc4random_buf"):
                    elf.check_symbols(f"1: 0000000000000000 0 {kind} GLOBAL DEFAULT UND arc4random_buf")

    def test_optimized_gnu_dependencies(self):
        elf = load_script("check-native-elf")
        output = "\n".join(
            f"(NEEDED) Shared library: [{library}]"
            for library in (
                "libc.so.6", "libdl.so.2", "libm.so.6",
                "libpthread.so.0", "librt.so.1", "libutil.so.1",
            )
        )
        for arch in ("aarch64", "x86_64"):
            target = f"{arch}-unknown-linux-gnu"
            elf.check_dependencies(output, target)
            for library in ("libstdc++.so.6", "libc++.so.1", "libcustom.so"):
                with self.subTest(target=target, library=library):
                    with self.assertRaises(ValueError):
                        elf.check_dependencies(output + f"\n(NEEDED) Shared library: [{library}]", target)

    def test_version_baseline(self):
        elf = load_script("check-native-elf")
        elf.check_versions("GLIBC_2.2.5 GLIBC_2.17 GLIBC_2.28")
        for output in ("GLIBC_2.29", "GLIBC_PRIVATE", "GLIBC_2.28 GLIBC_2.34", ""):
            with self.assertRaises(ValueError):
                elf.check_versions(output)

    def test_dependencies(self):
        elf = load_script("check-native-elf")
        elf.check_dependencies("(NEEDED) Shared library: [libc.so.6]", "x86_64-unknown-linux-gnu")
        elf.check_dependencies("(NEEDED) Shared library: [libc.so]", "aarch64-unknown-linux-musl")
        for target in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-musl"):
            for library in ("libstdc++.so.6", "libcustom.so"):
                with self.assertRaises(ValueError):
                    elf.check_dependencies(f"(NEEDED) Shared library: [{library}]", target)
        with self.assertRaises(ValueError):
            elf.check_dependencies("(NEEDED) [libc.so.6]\n(RUNPATH) [/build/lib]", "x86_64-unknown-linux-gnu")
