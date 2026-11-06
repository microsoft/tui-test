use std::path::Path;

fn main() {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock: toml::Value =
        toml::from_str(&std::fs::read_to_string(&lock_path).expect("read workspace Cargo.lock"))
            .expect("parse workspace Cargo.lock");
    let packages = lock["package"].as_array().expect("Cargo.lock packages");

    for name in [
        "alacritty_terminal",
        "libghostty-vt",
        "portable-pty",
        "rio-vt",
    ] {
        let mut matches = packages
            .iter()
            .filter(|package| package["name"].as_str() == Some(name));
        let package = matches
            .next()
            .unwrap_or_else(|| panic!("missing {name} in Cargo.lock"));
        assert!(
            matches.next().is_none(),
            "multiple versions of {name} in Cargo.lock; select the CLI's resolved dependency"
        );
        let version = package["version"].as_str().expect("package version");
        let key = name.replace('-', "_").to_ascii_uppercase();
        println!("cargo:rustc-env={key}_VERSION={version}");
    }
}
