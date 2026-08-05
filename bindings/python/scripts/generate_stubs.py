import argparse
import subprocess
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if src/shell_use/_native.pyi is out of date",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    command = [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(root / "stub-gen" / "Cargo.toml"),
    ]
    if args.check:
        command.extend(["--", "--check"])
    subprocess.run(
        command,
        cwd=str(root),
        check=True,
    )


if __name__ == "__main__":
    main()
