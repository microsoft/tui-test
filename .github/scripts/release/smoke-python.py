import asyncio
import os
import shutil
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
    if len(wheels) != 1:
        raise RuntimeError(f"Expected one Python wheel in dist, found {len(wheels)}")
    wheel = wheels[0]
    if "abi3" not in wheel.name:
        raise RuntimeError(f"Expected an abi3 wheel, found {wheel.name}")

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
            wheel,
        ],
        check=True,
    )

    runtime_env = os.environ.copy()
    runtime_env["SHELL_USE_BIN"] = str(
        (smoke_directory / "missing-shell-use").resolve()
    )
    if os.name == "nt":
        runtime_path = [str(python.parent)]
        system_root = runtime_env.get("SystemRoot")
        if system_root:
            runtime_path.extend(
                [str(Path(system_root) / "System32"), str(Path(system_root))]
            )
    else:
        runtime_path = [str(python.parent), "/usr/bin", "/bin"]
    runtime_env["PATH"] = os.pathsep.join(runtime_path)
    if shutil.which("shell-use", path=runtime_env["PATH"]) is not None:
        raise RuntimeError("shell-use CLI unexpectedly available in smoke PATH")

    subprocess.run(
        [python, Path(__file__).resolve(), "--run-smoke"],
        check=True,
        env=runtime_env,
    )


if __name__ == "__main__":
    main()
