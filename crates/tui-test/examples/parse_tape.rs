//! Run with `cargo run -p tui-test-rs --example parse_tape`.
//! An optional path replaces the built-in fixture. This example never runs a tape.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let tape = if let Some(path) = path {
        tui_test::tape::load(path, std::env::current_dir()?)?
    } else {
        tui_test::tape::parse(
            "tests/fixtures/tape/parse.tape",
            include_str!("../tests/fixtures/tape/parse.tape"),
        )?
    };
    let plan = tui_test::tape::compile(&tape)?;
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}
