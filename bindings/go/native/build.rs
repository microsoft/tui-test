fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../internal/native/native.h");
    println!("cargo:rerun-if-env-changed=TUI_TEST_GO_UPDATE_HEADER");
    let bindings = cbindgen::Builder::new()
        .with_config(cbindgen::Config {
            usize_is_size_t: true,
            ..Default::default()
        })
        .with_crate(&root)
        .with_language(cbindgen::Language::C)
        .with_include_guard("TUI_TEST_GO_NATIVE_H")
        .with_pragma_once(true)
        .with_documentation(true)
        .generate()
        .expect("generate Go native header");
    let mut generated = Vec::new();
    bindings.write(&mut generated);
    let header = root.join("../internal/native/native.h");
    if std::env::var_os("TUI_TEST_GO_UPDATE_HEADER").is_some() {
        std::fs::write(&header, &generated).expect("write Go native header");
    } else {
        let existing =
            std::fs::read(&header).expect("native.h missing; run with TUI_TEST_GO_UPDATE_HEADER=1");
        assert!(
            existing == generated,
            "native.h is stale; run with TUI_TEST_GO_UPDATE_HEADER=1"
        );
    }
}
