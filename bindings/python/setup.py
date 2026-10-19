"""Build the ordinary shared library with setuptools-rust, without Python linkage."""

import os
from pathlib import Path
import re
import shutil

from setuptools import setup
from setuptools.command.sdist import sdist
from setuptools_rust import Binding, RustExtension
from setuptools_rust.build import build_rust
from wheel.bdist_wheel import bdist_wheel


ROOT = Path(__file__).resolve().parent
RUST_ROOT = ROOT / "_rust" if (ROOT / "_rust").is_dir() else ROOT.parent.parent


class SharedLibraryBuild(build_rust):
    def get_dylib_ext_path(self, ext, target_fname):
        extension_path = Path(super().get_dylib_ext_path(ext, target_fname))
        platform = self.plat_name.lower()
        filename = (
            "tui_test_ffi.dll" if platform.startswith("win")
            else "libtui_test_ffi.dylib" if platform.startswith("macosx")
            else "libtui_test_ffi.so"
        )
        return str(extension_path.with_name(filename))


class SharedLibraryWheel(bdist_wheel):
    def get_tag(self):
        _, _, platform = super().get_tag()
        return "py3", "none", platform


class SourceDistribution(sdist):
    def make_release_tree(self, base_dir, files):
        super().make_release_tree(base_dir, files)
        destination = Path(base_dir) / "_rust"
        if destination.is_dir():
            return
        destination.mkdir()
        cargo_config = Path(base_dir) / ".cargo"
        cargo_config.mkdir(exist_ok=True)
        shutil.copy2(RUST_ROOT / ".cargo/config.toml", cargo_config / "config.toml")
        for filename in ("Cargo.lock", "LICENSE", "README.md"):
            shutil.copy2(RUST_ROOT / filename, destination / filename)
        manifest = (RUST_ROOT / "Cargo.toml").read_text(encoding="utf-8")
        # An sdist needs only the shared ABI and its engine, not the CLI or other bindings.
        for key in ("members", "default-members"):
            manifest = re.sub(
                r"(?m)^" + key + r"\s*=\s*\[[^\]]*\]",
                key + ' = ["crates/tui-test", "crates/tui-test-ffi"]',
                manifest,
            )
        (destination / "Cargo.toml").write_text(manifest, encoding="utf-8")
        for crate in ("tui-test", "tui-test-ffi"):
            source = RUST_ROOT / "crates" / crate
            target = destination / "crates" / crate
            target.mkdir(parents=True)
            shutil.copy2(source / "Cargo.toml", target / "Cargo.toml")
            if (source / "build.rs").is_file():
                shutil.copy2(source / "build.rs", target / "build.rs")
            for directory in ("src", "assets", "shell", "include"):
                if (source / directory).is_dir():
                    shutil.copytree(source / directory, target / directory)


setup(
    rust_extensions=[
        RustExtension(
            "tui_test.tui_test_ffi",
            path=os.path.relpath(RUST_ROOT / "crates/tui-test-ffi/Cargo.toml", ROOT),
            binding=Binding.NoBinding,
        ),
    ],
    cmdclass={
        "build_rust": SharedLibraryBuild,
        "bdist_wheel": SharedLibraryWheel,
        "sdist": SourceDistribution,
    },
    zip_safe=False,
)
