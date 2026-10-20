#!/usr/bin/env python3
"""Check Linux shared library dependencies and the GNU glibc 2.28 baseline."""

import argparse
import re
import subprocess
from pathlib import Path


def check_versions(output):
    versions = re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", output)
    if not versions or "GLIBC_PRIVATE" in output:
        raise ValueError("Missing or unsupported glibc symbol versions")
    newer = sorted({version for version in versions if tuple(map(int, version.split("."))) > (2, 28)})
    if newer:
        raise ValueError(f"Library requires glibc newer than 2.28: {', '.join(newer)}")


def check_dependencies(output, target):
    if target.endswith("-gnu"):
        allowed = {
            "libc.so.6", "libdl.so.2", "libpthread.so.0", "libm.so.6", "librt.so.1", "libutil.so.1",
            "libgcc_s.so.1", "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1",
        }
    else:
        arch = "aarch64" if target.startswith("aarch64-") else "x86_64"
        allowed = {"libc.so", f"libc.musl-{arch}.so.1"}
    needed = set(re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", output))
    if not needed or needed - allowed:
        raise ValueError(f"Unexpected native dependencies for {target}: {sorted(needed)}")
    if re.search(r"\((?:RPATH|RUNPATH)\)", output):
        raise ValueError("Native release library must not contain RPATH or RUNPATH")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--target", required=True, choices=[
        f"{arch}-unknown-linux-{libc}"
        for arch in ("x86_64", "aarch64") for libc in ("gnu", "musl")
    ])
    args = parser.parse_args()
    dynamic = subprocess.check_output(["readelf", "--dynamic", str(args.library)], text=True)
    check_dependencies(dynamic, args.target)
    if args.target.endswith("-gnu"):
        versions = subprocess.check_output(["readelf", "--version-info", str(args.library)], text=True)
        check_versions(versions)
    print(f"Native dependencies and ABI checks passed: {args.target}")


if __name__ == "__main__":
    main()
