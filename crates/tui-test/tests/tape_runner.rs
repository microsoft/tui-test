use tui_test::tape::{check, compile, parse, run, RunnerPhase};

#[test]
fn public_preflight_rejects_missing_raster_or_directory_without_output() {
    let plan = compile(&parse("public.tape", "Output public.gif Sleep .1").unwrap()).unwrap();
    let cwd = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tape-runner-directory-that-does-not-exist");
    let error = check(&plan, &cwd).unwrap_err();
    assert_eq!(error.phase, RunnerPhase::Preflight);
    assert_eq!(error.kind, tui_test::ErrorKind::Usage);
    assert_eq!(run(&plan, &cwd).unwrap_err().phase, RunnerPhase::Preflight);
    assert!(!cwd.exists());
}

#[cfg(feature = "recording-font-jetbrains-mono")]
#[test]
fn public_runner_exports_a_decodable_gif_and_keeps_plan_immutable() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("tape-public-api-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let plan = compile(&parse("public.tape",
        "Output result.gif Set Rows 3 Set Columns 20 Set Padding 4 Set FontSize 12 Set CursorBlink false Sleep .2",
    ).unwrap()).unwrap();
    let original = serde_json::to_value(&plan).unwrap();
    let expected = check(&plan, &directory).unwrap();
    assert!(!expected.output.exists());
    let started = std::time::Instant::now();
    let actual = run(&plan, &directory).unwrap();
    let elapsed = started.elapsed();
    assert_eq!(actual, expected);
    assert_eq!(serde_json::to_value(&plan).unwrap(), original);
    let mut decoder = gif::DecodeOptions::new()
        .read_info(std::fs::File::open(&actual.output).unwrap())
        .unwrap();
    assert_eq!(u32::from(decoder.width()), actual.geometry.width);
    assert_eq!(u32::from(decoder.height()), actual.geometry.height);
    let mut duration = 0u64;
    let mut count = 0;
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        count += 1;
        duration += u64::from(frame.delay);
    }
    assert!(count > 0);
    let recorded = std::time::Duration::from_millis(duration * 10);
    let quantum = std::time::Duration::from_millis(10);
    assert!(
        recorded + quantum >= std::time::Duration::from_millis(200)
            && recorded <= elapsed + quantum,
        "GIF duration: {duration} cs; elapsed: {elapsed:?}"
    );
    drop(decoder);
    std::fs::remove_dir_all(directory).unwrap();
}
