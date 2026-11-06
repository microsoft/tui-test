#!/usr/bin/env python3
"""Vendor the signed ConPTY package without adding a build-time download."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import struct
import urllib.request
import zipfile


PACKAGE = "Microsoft.Windows.Console.ConPTY"
FEED = (
    "https://pkgs.dev.azure.com/shine-oss/"
    "7d07a4e8-65aa-48e9-90cf-7de6554e00a4/_packaging/"
    "6a7d42dc-65dc-4cc2-8b76-76fa4aae706a/nuget/v3/flat2/"
)
ASSETS = Path(__file__).resolve().parents[2] / "crates" / "tui-test" / "assets" / "conpty"


def file_version(data):
    signature = b"\xbd\x04\xef\xfe\x00\x00\x01\x00"
    if data.count(signature) != 1:
        raise ValueError("expected one VS_FIXEDFILEINFO resource")
    offset = data.index(signature)
    ms, ls = struct.unpack_from("<II", data, offset + 8)
    return f"{ms >> 16}.{ms & 0xffff}.{ls >> 16}.{ls & 0xffff}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--sha256", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"\d+(?:\.\d+)+", args.version):
        parser.error("version must contain dot-separated integers")
    if not re.fullmatch(r"[0-9a-f]{64}", args.sha256):
        parser.error("sha256 must be 64 lowercase hexadecimal characters")
    name = PACKAGE.lower()
    url = f"{FEED}{name}/{args.version}/{name}.{args.version}.nupkg"
    with urllib.request.urlopen(url, timeout=120) as response:
        package = response.read()
    if hashlib.sha256(package).hexdigest() != args.sha256:
        raise ValueError("NuGet package checksum mismatch")

    entries = []
    versions = set()
    files = []
    with zipfile.ZipFile(io.BytesIO(package)) as archive:
        for architecture, machine in [("x86", 0x14c), ("x64", 0x8664), ("arm64", 0xaa64)]:
            for filename, source in [
                ("conpty.dll", f"runtimes/win10-{architecture}/native/conpty.dll"),
                ("OpenConsole.exe", f"build/native/runtimes/{architecture}/OpenConsole.exe"),
            ]:
                data = archive.read(source)
                pe = struct.unpack_from("<I", data, 0x3c)[0]
                if data[pe:pe + 4] != b"PE\0\0" or struct.unpack_from("<H", data, pe + 4)[0] != machine:
                    raise ValueError(f"unexpected PE architecture for {source}")
                versions.add(file_version(data))
                relative = f"{architecture}/{filename}"
                destination = ASSETS / architecture / f"{filename}.gz"
                compressed = io.BytesIO()
                with gzip.GzipFile(
                    filename="", mode="wb", fileobj=compressed, compresslevel=6, mtime=0
                ) as stream:
                    stream.write(data)
                files.append((destination, compressed.getvalue()))
                entries.append({
                    "path": relative,
                    "size": len(data),
                    "sha256": hashlib.sha256(data).hexdigest(),
                })
    if len(versions) != 1:
        raise ValueError("ConPTY DLL and hosts have different file versions")
    for destination, data in files:
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    manifest = {
        "package": PACKAGE,
        "version": args.version,
        "file_version": versions.pop(),
        "source": url,
        "package_sha256": args.sha256,
        "assets": entries,
    }
    (ASSETS / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Vendored {PACKAGE} {args.version} into {ASSETS}")


if __name__ == "__main__":
    main()
