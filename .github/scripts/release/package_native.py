"""Package native engines and Go modules from the current release checkout."""

import argparse
import gzip
import hashlib
import io
import json
import re
import struct
import subprocess
import tarfile
from pathlib import Path


MAX_MODULE_SIZE = 500 * 1024 * 1024


def release_metadata(root):
    package = json.loads((root / "bindings/js/package.json").read_text())
    version = package["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-beta\.\d+)?", version):
        raise ValueError(f"Unsupported release version: {version}")
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True
    ).strip()
    return version, revision, package["nativeTargets"]


def library_name(target):
    if "windows" in target:
        return "tui_test_ffi.dll"
    if "apple" in target:
        return "libtui_test_ffi.dylib"
    return "libtui_test_ffi.so"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def validate_library(data, target):
    """Check the platform and architecture, not the runtime ABI baseline."""
    arm = target.startswith("aarch64-")
    try:
        if "linux" in target:
            valid = (
                data[:6] == b"\x7fELF\x02\x01"
                and struct.unpack_from("<HH", data, 16) == (3, 183 if arm else 62)
            )
        elif "apple" in target:
            valid = (
                struct.unpack_from("<II", data) == (0xFEEDFACF, 0x100000C if arm else 0x1000007)
                and struct.unpack_from("<I", data, 12)[0] == 6
            )
        else:
            offset = struct.unpack_from("<I", data, 60)[0]
            valid = (
                data[:2] == b"MZ"
                and data[offset:offset + 4] == b"PE\0\0"
                and struct.unpack_from("<H", data, offset + 4)[0] == 0x8664
                and struct.unpack_from("<H", data, offset + 22)[0] & 0x2000
            )
    except struct.error:
        valid = False
    if not valid:
        raise ValueError(f"Library format or architecture does not match {target}")


def archive_bytes(entries):
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, data in sorted(entries.items()):
                member = tarfile.TarInfo(name)
                member.size = len(data)
                member.mode = 0o644
                archive.addfile(member, io.BytesIO(data))
    return buffer.getvalue()


def write_archive(path, entries):
    data = archive_bytes(entries)
    path.write_bytes(data)
    path.with_name(path.name + ".sha256").write_text(f"{sha256(data)}  {path.name}\n")


def native_filename(version, target):
    return f"tui-test-native-{version}-{target}.tar.gz"


def read_native(data, version, revision, target):
    library = library_name(target)
    with gzip.GzipFile(fileobj=io.BytesIO(data), mode="rb") as compressed, \
            tarfile.open(fileobj=compressed, mode="r:") as archive:
        members = archive.getmembers()
        if (
            len(members) != 3
            or {member.name for member in members} != {library, "LICENSE", "manifest.json"}
            or any(not member.isfile() or member.size > MAX_MODULE_SIZE for member in members)
        ):
            raise ValueError(f"Unsafe or unexpected archive members for {target}")
        contents = {member.name: archive.extractfile(member).read() for member in members}
        # Tar EOF can precede the gzip trailer that checks CRC and stream length.
        while compressed.read(64 * 1024):
            pass
    manifest = json.loads(contents["manifest.json"])
    expected = {
        "version": version,
        "revision": revision,
        "target": target,
        "library": library,
        "sha256": sha256(contents[library]),
    }
    if manifest != expected:
        raise ValueError(f"Native manifest or checksum mismatch for {target}")
    validate_library(contents[library], target)
    return manifest


def native_archives(root, artifacts, targets=None):
    version, revision, configured = release_metadata(root)
    selected = list(configured) if targets is None else targets
    if not selected or len(set(selected)) != len(selected) or set(selected) - set(configured):
        raise ValueError("Select unique, configured native targets")
    expected = {native_filename(version, target) for target in selected}
    if targets is None and {path.name for path in artifacts.glob("*.tar.gz")} != expected:
        raise ValueError("Expected exactly the seven configured native archives for this release")
    archives = {}
    for target in selected:
        path = artifacts / native_filename(version, target)
        data = path.read_bytes()
        checksum = path.with_name(path.name + ".sha256")
        if checksum.exists() and checksum.read_text() != f"{sha256(data)}  {path.name}\n":
            raise ValueError(f"Archive checksum mismatch: {path.name}")
        manifest = read_native(data, version, revision, target)
        archives[target] = (data, manifest)
    return archives


def module_entries(root, archives):
    entries = {}
    source = root / "bindings/go"
    for path in sorted(source.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"Go module contains a symbolic link: {path}")
        if path.is_file():
            name = path.relative_to(source).as_posix()
            if name.startswith("internal/native/bundled/") or (
                name.startswith("internal/native/bundled_") and name.endswith(".go")
            ):
                raise ValueError(f"Source module contains generated release files: {name}")
            entries[name] = path.read_bytes()
    entries["LICENSE"] = (root / "LICENSE").read_bytes()
    _, _, configured = release_metadata(root)
    platforms = {}
    for target, (data, manifest) in sorted(archives.items()):
        platform, arch, *_ = configured[target].split("-")
        platform = {"win32": "windows"}.get(platform, platform)
        arch = {"x64": "amd64", "arm64": "arm64"}[arch]
        platforms.setdefault((platform, arch), []).append((target, manifest))
        entries[f"internal/native/bundled/{target}.tar.gz"] = data
    for (platform, arch), libraries in platforms.items():
        lines = [
            "// Code generated by package_native.py. DO NOT EDIT.",
            f"//go:build {platform} && {arch}",
            "",
            "package native",
            "",
            'import _ "embed"',
            "",
        ]
        for target, _ in libraries:
            identifier = "bundledArchive_" + target.replace("-", "_")
            lines += [
                f"//go:embed bundled/{target}.tar.gz",
                f"var {identifier} []byte",
                "",
            ]
        lines.append("func init() {")
        for target, manifest in libraries:
            identifier = "bundledArchive_" + target.replace("-", "_")
            lines += [
                f'\tbundledLibraries["{target}"] = bundledLibrary{{',
                f'\t\tname:     "{manifest["library"]}",',
                f'\t\tchecksum: "{manifest["sha256"]}",',
                f"\t\tarchive:  {identifier},",
                "\t}",
            ]
        lines += ["}", ""]
        entries[f"internal/native/bundled_{platform}_{arch}.go"] = "\n".join(lines).encode()
    if sum(map(len, entries.values())) >= MAX_MODULE_SIZE:
        raise ValueError("Go module must be smaller than 500 MiB uncompressed")
    return entries


def directory_entries(directory):
    entries = {}
    for path in directory.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"Package contains a symbolic link: {path}")
        if path.is_file():
            entries[path.relative_to(directory).as_posix()] = path.read_bytes()
    return entries


def verify_module(root, directory):
    version, revision, targets = release_metadata(root)
    archives = {}
    for target in targets:
        data = (directory / f"internal/native/bundled/{target}.tar.gz").read_bytes()
        archives[target] = (data, read_native(data, version, revision, target))
    expected = module_entries(root, archives)
    if directory_entries(directory) != expected:
        raise ValueError("Go module does not match the source revision and native archives")
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    native = commands.add_parser("native", help="Create a native archive and checksum")
    native.add_argument("--library", type=Path, required=True)
    native.add_argument("--target", required=True)
    native.add_argument("--output", type=Path, default=Path("target/native-packages"))
    go = commands.add_parser("go", help="Assemble a Go module and its archive")
    go.add_argument("--artifacts", type=Path, required=True)
    go.add_argument("--output", type=Path, default=Path("target/go-package"))
    go.add_argument("--target", action="append", help="Select targets for local tests only")
    verify = commands.add_parser("verify", help="Verify the complete module before publication")
    verify.add_argument("--package", type=Path, default=Path("target/go-package"))
    verify.add_argument("--version", required=True)
    args = parser.parse_args()
    root = Path.cwd()
    version, revision, targets = release_metadata(root)

    if args.command == "native":
        if args.target not in targets:
            parser.error(f"Unsupported target: {args.target}")
        data = args.library.read_bytes()
        validate_library(data, args.target)
        library = library_name(args.target)
        manifest = {
            "version": version, "revision": revision, "target": args.target,
            "library": library, "sha256": sha256(data),
        }
        args.output.mkdir(parents=True, exist_ok=True)
        path = args.output / native_filename(version, args.target)
        write_archive(path, {
            library: data,
            "LICENSE": (root / "LICENSE").read_bytes(),
            "manifest.json": (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
        })
        print(path)
    elif args.command == "go":
        output = args.output.resolve()
        source = (root / "bindings/go").resolve()
        if output == source or source in output.parents:
            parser.error("Output must be outside bindings/go")
        if output.exists():
            parser.error("Output must not exist")
        entries = module_entries(root, native_archives(root, args.artifacts, args.target))
        output.mkdir(parents=True)
        for name, data in entries.items():
            path = output / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        archive = output.parent / f"tui-test-go-{version}.tar.gz"
        write_archive(archive, entries)
        print(output)
        print(archive)
    else:
        if args.version != version:
            parser.error("Release tag does not match the package version")
        verify_module(root, args.package)
        print(f"Verified Go module v{version} from {revision}")


if __name__ == "__main__":
    main()
