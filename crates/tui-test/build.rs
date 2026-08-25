//! Resolves the xterm.js bundles the `xtermjs` feature compiles in.
//!
//! The bundles are not checked into the repository. They are fetched from npm
//! at the versions `assets/xtermjs/pinned.json` names, and cached under
//! `OUT_DIR` so the fetch happens once per target directory rather than once
//! per build.
//!
//! A published crate carries the bundles alongside this file, because
//! `cargo package` puts them in the tarball. That case is checked first, so
//! building a release of `tui-test-rs` needs neither npm nor a network — only
//! building from a repository checkout does.

use std::path::PathBuf;

/// One vendored bundle: the npm package it comes from, the path inside that
/// package's tarball, and the file it is written to.
struct Bundle {
    package: &'static str,
    inner: &'static str,
    file: &'static str,
    env: &'static str,
}

const BUNDLES: [Bundle; 2] = [
    Bundle {
        package: "@xterm/headless",
        inner: "package/lib-headless/xterm-headless.js",
        file: "xterm-headless.js",
        env: "XTERM_HEADLESS_JS",
    },
    Bundle {
        package: "@xterm/addon-unicode11",
        inner: "package/lib/addon-unicode11.js",
        file: "addon-unicode11.js",
        env: "XTERM_UNICODE11_JS",
    },
];

fn main() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/xtermjs");
    println!(
        "cargo:rerun-if-changed={}",
        assets.join("pinned.json").display()
    );
    println!("cargo:rerun-if-changed=build.rs");

    // Only the `xtermjs` feature compiles the bundles in. Every other build
    // skips the fetch entirely.
    if std::env::var_os("CARGO_FEATURE_XTERMJS").is_none() {
        for bundle in &BUNDLES {
            println!(
                "cargo:rustc-env={}={}",
                bundle.env,
                assets.join(bundle.file).display()
            );
        }
        return;
    }

    let pinned = std::fs::read_to_string(assets.join("pinned.json"))
        .unwrap_or_else(|error| panic!("read {}: {error}", assets.join("pinned.json").display()));

    for bundle in &BUNDLES {
        let version = pinned_version(&pinned, bundle.package).unwrap_or_else(|| {
            panic!(
                "{} does not name a version for {}",
                assets.join("pinned.json").display(),
                bundle.package
            )
        });

        // A published crate ships the bundle next to this script.
        let vendored = assets.join(bundle.file);
        let resolved = if vendored.is_file() {
            println!("cargo:rerun-if-changed={}", vendored.display());
            vendored
        } else {
            fetch(bundle, &version)
        };

        println!("cargo:rustc-env={}={}", bundle.env, resolved.display());
    }
}

/// Read one `"package": "version"` pair out of `pinned.json`.
///
/// Hand-parsed rather than pulled from a JSON crate: a build dependency is
/// compiled for the host on every clean build of every downstream crate, and
/// this file has two keys.
fn pinned_version(pinned: &str, package: &str) -> Option<String> {
    let key = format!("\"{package}\"");
    let rest = pinned.split_once(&key)?.1;
    let rest = rest.split_once(':')?.1;
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(rest[start..end].to_string())
}

/// Fetch one bundle from npm into `OUT_DIR`, and return where it landed.
fn fetch(bundle: &Bundle, version: &str) -> PathBuf {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    // Keyed by version so a bump re-fetches rather than reusing the old bytes.
    let cached = out.join(format!("{version}-{}", bundle.file));
    if cached.is_file() {
        return cached;
    }

    let work = out.join(format!("fetch-{version}-{}", bundle.file));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)
        .unwrap_or_else(|error| panic!("create {}: {error}", work.display()));

    let spec = format!("{}@{}", bundle.package, version);
    run(
        std::process::Command::new(npm())
            .args(["pack", &spec, "--silent"])
            .current_dir(&work),
        &format!("npm pack {spec}"),
    );

    let tarball = std::fs::read_dir(&work)
        .unwrap_or_else(|error| panic!("read {}: {error}", work.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "tgz"))
        .unwrap_or_else(|| panic!("npm pack {spec} produced no tarball in {}", work.display()));

    run(
        std::process::Command::new("tar")
            .arg("xzf")
            .arg(&tarball)
            .arg(bundle.inner)
            .current_dir(&work),
        &format!("extract {} from {}", bundle.inner, tarball.display()),
    );

    let extracted = work.join(bundle.inner);
    std::fs::copy(&extracted, &cached).unwrap_or_else(|error| {
        panic!(
            "copy {} to {}: {error}",
            extracted.display(),
            cached.display()
        )
    });
    let _ = std::fs::remove_dir_all(&work);
    cached
}

/// `npm` is a shell script on Unix and a `.cmd` on Windows, which
/// `Command::new` cannot execute directly.
fn npm() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

fn run(command: &mut std::process::Command, what: &str) {
    let output = command.output().unwrap_or_else(|error| {
        panic!(
            "{what} failed to start: {error}\n\
             The xtermjs feature fetches its bundles from npm at build time, so \
             building it from a repository checkout needs npm and a network. A \
             published release of tui-test-rs carries the bundles and needs neither."
        )
    });
    if !output.status.success() {
        panic!(
            "{what} failed with {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::pinned_version;

    #[test]
    fn reads_a_version_out_of_pinned_json() {
        let pinned = r#"{
  "@xterm/headless": "6.0.0",
  "@xterm/addon-unicode11": "0.9.0"
}"#;
        assert_eq!(
            pinned_version(pinned, "@xterm/headless").as_deref(),
            Some("6.0.0")
        );
        assert_eq!(
            pinned_version(pinned, "@xterm/addon-unicode11").as_deref(),
            Some("0.9.0")
        );
        assert_eq!(pinned_version(pinned, "@xterm/nope"), None);
    }
}
