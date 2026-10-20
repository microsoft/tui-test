"""Build an external Go consumer and run it without native build tools."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    args = parser.parse_args()
    package = args.package.resolve()
    root = Path(__file__).resolve().parents[2]
    target = root / "target"
    target.mkdir(exist_ok=True)
    go = shutil.which("go")
    if go is None:
        raise SystemExit("go is required to build the consumer")
    with tempfile.TemporaryDirectory(prefix="go-consumer-", dir=target) as temporary:
        directory = Path(temporary)
        goroot = subprocess.check_output([go, "env", "GOROOT"], text=True).strip()
        environment = dict(
            os.environ, CGO_ENABLED="0", GOWORK="off", GOTOOLCHAIN="local",
            GOROOT=goroot, PATH="",
        )
        environment.pop("TUI_TEST_NATIVE_LIBRARY", None)
        environment.pop("GOFLAGS", None)
        environment.pop("GOOS", None)
        environment.pop("GOARCH", None)
        for command in (
            ["mod", "init", "example.com/tui-test-consumer"],
            ["mod", "edit", f"-replace=github.com/microsoft/tui-test/bindings/go={package}"],
            ["get", "github.com/microsoft/tui-test/bindings/go@v0.0.0"],
            ["build", "-buildvcs=false", "-o", str(directory / "smoke.exe"),
             "github.com/microsoft/tui-test/bindings/go/examples/smoke"],
        ):
            subprocess.run([go, *command], cwd=directory, env=environment, check=True)

        runtime = dict(
            environment,
            PATH="",
            HOME=str(directory / "home"),
            XDG_CACHE_HOME=str(directory / "cache"),
            LOCALAPPDATA=str(directory / "cache"),
        )
        for attempt in ("first extraction", "cache reuse"):
            result = subprocess.run(
                [str(directory / "smoke.exe")], cwd=directory, env=runtime,
                check=True, capture_output=True, text=True, timeout=60,
            )
            if "Go binding smoke passed (bundled engine)" not in result.stdout:
                raise RuntimeError(f"unexpected consumer output: {result.stdout!r}")
            print(f"{attempt}: {result.stdout.strip()}")

        readme = (package / "README.md").read_text(encoding="utf-8")
        example = readme.split("```go\n", 1)[1].split("\n```", 1)[0]
        (directory / "main.go").write_text(example + "\n", encoding="utf-8")
        subprocess.run(
            [go, "build", "-buildvcs=false", "-o", str(directory / "readme.exe"), "."],
            cwd=directory, env=environment, check=True,
        )
        result = subprocess.run(
            [str(directory / "readme.exe")], cwd=directory,
            env=dict(runtime, PATH=os.environ.get("PATH", "")),
            check=True, capture_output=True, text=True, timeout=60,
        )
        if "hello" not in result.stdout:
            raise RuntimeError(f"README example failed: {result.stdout!r}\n{result.stderr}")
        print(f"README quick start: {result.stdout.strip()}")


if __name__ == "__main__":
    main()
