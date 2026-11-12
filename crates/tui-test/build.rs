use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=TUI_TEST_WINDOWS_TERMINAL_LIB_DIR");
    println!("cargo:rerun-if-changed=native/windows-terminal");
    if env::var_os("CARGO_FEATURE_WINDOWS_TERMINAL").is_none()
        || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
    {
        return;
    }

    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "the windows-terminal feature requires the MSVC target"
    );
    assert!(
        env::var("CARGO_CFG_TARGET_FEATURE")
            .unwrap_or_default()
            .split(',')
            .any(|feature| feature == "crt-static"),
        "the windows-terminal bridge requires -C target-feature=+crt-static"
    );
    let architecture = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => panic!("the windows-terminal backend does not support {other}"),
    };
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let library_dir = env::var_os("TUI_TEST_WINDOWS_TERMINAL_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            manifest
                .join("../../target/windows-terminal")
                .join(architecture)
        });
    let instructions = "Run scripts\\build-windows-terminal.ps1 first (use -Architecture ARM64 \
        when cross-compiling), or set TUI_TEST_WINDOWS_TERMINAL_LIB_DIR to its output directory. \
        See crates\\tui-test\\native\\windows-terminal\\README.md.";
    let receipt_path = library_dir.join("build-info.txt");
    println!("cargo:rerun-if-changed={}", receipt_path.display());
    let receipt = fs::read_to_string(&receipt_path)
        .unwrap_or_else(|error| panic!("{}: {error}. {instructions}", receipt_path.display()));
    let revision = include_str!("native/windows-terminal/upstream-revision.txt").trim();
    for field in [
        format!("upstream={revision}"),
        format!("architecture={architecture}"),
        "vcpkg=927f62e4b8838bd7e441e9c45103a16ffd75007e".to_string(),
        "abi=1".to_string(),
        "crt=static".to_string(),
    ] {
        assert!(
            receipt.lines().any(|line| line == field),
            "incompatible Windows Terminal native build: missing {field:?}. {instructions}"
        );
    }
    for input in [
        "bridge.h",
        "bridge.cpp",
        "bridge.vcxproj",
        "headless-core.vcxproj",
        "packages.config",
        "upstream-revision.txt",
    ] {
        let current = fs::read(manifest.join("native/windows-terminal").join(input)).unwrap();
        let compiled = fs::read(library_dir.join("inputs").join(input))
            .unwrap_or_else(|error| panic!("missing build input {input}: {error}. {instructions}"));
        assert!(
            current == compiled,
            "stale Windows Terminal native build ({input} changed). {instructions}"
        );
    }
    let library = library_dir.join("tui_test_windows_terminal.lib");
    assert!(
        library.is_file(),
        "{} is missing. {instructions}",
        library.display()
    );
    println!("cargo:rerun-if-changed={}", library.display());
    println!("cargo:rustc-link-search=native={}", library_dir.display());
    println!("cargo:rustc-link-lib=static=tui_test_windows_terminal");
    for library in [
        "windowsapp",
        "onecoreuap_apiset",
        "dwrite",
        "d3dcompiler",
        "dwmapi",
        "uxtheme",
        "shlwapi",
        "ntdll",
        "icu",
    ] {
        println!("cargo:rustc-link-lib={library}");
    }
}
