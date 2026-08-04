import asyncio
import os
import subprocess
import sys
import venv
from pathlib import Path


async def smoke_test():
    from shell_use import ShellUse

    async with ShellUse.ephemeral("release-smoke") as session:
        await session.open()
        await session.submit("echo release-smoke")
        await session.wait_command()
        await session.expect_text("release-smoke", strict=False)


def main():
    if "--run-smoke" in sys.argv:
        asyncio.run(smoke_test())
        return

    wheels = sorted(Path("dist").glob("*.whl"))
    if not wheels:
        raise RuntimeError("No Python wheels found in dist")

    smoke_directory = Path("smoke")
    venv.create(smoke_directory, with_pip=True)
    python = smoke_directory / (
        "Scripts/python.exe" if os.name == "nt" else "bin/python"
    )

    subprocess.run(
        [
            python,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            *wheels,
        ],
        check=True,
    )
    subprocess.run(
        [python, Path(__file__).resolve(), "--run-smoke"],
        check=True,
    )


if __name__ == "__main__":
    main()
