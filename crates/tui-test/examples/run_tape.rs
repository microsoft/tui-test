//! Run with `cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example run_tape`.
//! An optional tape path replaces the fixture. Output paths use the invocation directory.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let tape = if let Some(path) = std::env::args_os().nth(1) {
        tui_test::tape::load(path, &cwd)?
    } else {
        tui_test::tape::parse(
            "tests/fixtures/tape/run.tape",
            include_str!("../tests/fixtures/tape/run.tape"),
        )?
    };
    let plan = tui_test::tape::compile(&tape)?;
    let summary = tui_test::tape::check(&plan, &cwd)?;
    println!("Preflight:\n{}", serde_json::to_string_pretty(&summary)?);
    let summary = tui_test::tape::run(&plan, &cwd)?;
    println!("Recorded {}", summary.output.display());
    Ok(())
}
