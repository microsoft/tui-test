//! U02 acceptance: the public owned snapshot over live sessions and offline input.
//!
//! Live content is produced by the child (`printf` at startup, then `sleep`),
//! so no PTY echo path can mangle escape sequences. Each capture is polled to
//! a known marker before asserting, so no assertion depends on timing.

#![cfg(unix)]

use std::time::{Duration, Instant};

use tui_test::profile::{Profile, Rgb};
use tui_test::terminal::cell::Attrs;
use tui_test::{
    OpenOptions, Operation, OperationResult, RunOptions, ScrollTarget, Session, SnapshotCell,
    SnapshotColor, SnapshotCursor, SnapshotCursorShape, SnapshotError, SnapshotMetadata,
    SnapshotOptions, SnapshotParts, SnapshotScope, SnapshotSource, SnapshotUnderline,
    TerminalSnapshot, MAX_CELL_TEXT_BYTES, MAX_SNAPSHOT_CELLS, MAX_SNAPSHOT_DIM,
};

fn session_with_script(test: &str, script: &str, cols: u16, rows: u16) -> Session {
    let session = Session::new(format!("owned-snapshot-{test}-{}", std::process::id()));
    let defaults = OpenOptions::default();
    session
        .run(RunOptions {
            backend: defaults.backend,
            program: "sh".to_string(),
            args: vec!["-c".to_string(), format!("printf '{script}'; sleep 30")],
            profile: defaults.profile,
            style: defaults.style,
            cols,
            rows,
            cwd: defaults.cwd,
            env: defaults.env,
            wait_ready: Some(false),
            restart: defaults.restart,
            timeouts: defaults.timeouts,
            recording: defaults.recording,
        })
        .expect("session starts");
    session
}

fn live_text(session: &Session) -> String {
    match session.execute(Operation::Text { full: false }) {
        Ok(OperationResult::Text(text)) => text,
        other => panic!("text operation gave {other:?}"),
    }
}

fn poll_text(session: &Session, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if live_text(session).contains(needle) {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for {needle:?}; got {:?}",
                live_text(session)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn close(session: &Session) {
    session.close().expect("session closes");
}

fn resolved_palette() -> [Rgb; 256] {
    let profile = Profile::default();
    std::array::from_fn(|index| profile.colors.rgb(index as u8))
}

fn blank_parts(cols: u32, rows: u32) -> SnapshotParts {
    let palette = resolved_palette();
    SnapshotParts {
        cols,
        rows,
        scope: SnapshotScope::Viewport,
        viewport_origin: 0,
        cells: (0..cols * rows)
            .map(|_| {
                SnapshotCell::new(
                    " ".to_string(),
                    1,
                    SnapshotColor::Default,
                    SnapshotColor::Default,
                    Attrs::empty(),
                    SnapshotUnderline::None,
                    SnapshotColor::Default,
                    None,
                    None,
                )
                .expect("blank cell is valid")
            })
            .collect(),
        palette,
        default_foreground: Rgb::new(1, 2, 3),
        default_background: Rgb::new(4, 5, 6),
        default_cursor: Rgb::new(7, 8, 9),
        cursor: SnapshotCursor::new(
            0,
            0,
            true,
            true,
            SnapshotCursorShape::Block,
            Rgb::new(7, 8, 9),
            None,
        ),
        title: None,
        metadata: SnapshotMetadata::default(),
    }
}

fn assert_rectangular(snapshot: &TerminalSnapshot) {
    assert_ne!((snapshot.cols(), snapshot.rows()), (0, 0));
    assert_eq!(
        snapshot.cells().len() as u64,
        u64::from(snapshot.cols()) * u64::from(snapshot.rows())
    );
    for row in 0..snapshot.rows() {
        assert_eq!(
            snapshot.row(row).expect("row exists").len() as u32,
            snapshot.cols(),
            "row {row} keeps rectangular geometry"
        );
    }
    if snapshot.cursor().in_capture() {
        let (col, row) = snapshot.cursor_local().expect("local cursor");
        assert!(col < snapshot.cols());
        assert!(row < snapshot.rows());
    }
}

/// U02-A: live transitions across the Unicode corpus, styles, palette,
/// defaults, underline, reverse, cursor, links, and title stay coherent.
#[test]
fn u02_a_live_transitions_stay_coherent() {
    let script = concat!(
        "\\033[2J\\033[H",
        "é ä 東 \\033[1mbold\\033[0m\\r\\n",
        "\\033[3mitalic\\033[0m \\033[1;3mbi\\033[0m\\r\\n",
        "\\033[31mR1\\033[38;5;2mR2\\033[38;2;9;9;9mR3\\033[0m\\r\\n",
        "\\033[4:3mU\\033[58;5;200mV\\033[7mW\\033[0m\\r\\n",
        "                                                                              東\\r\\n",
        "\\033]4;7;#123456\\033\\\\",
        "\\033]10;#111111\\033\\\\\\033]11;#222222\\033\\\\\\033]12;#333333\\033\\\\",
        "\\033]8;id=live;https://example.com/live\\033\\\\LINK\\033]8;;\\033\\\\\\r\\n",
        "\\033]0;live-title\\033\\\\",
        "\\033[5;79H",
    );
    let session = session_with_script("a", script, 80, 24);
    poll_text(&session, "LINK");

    let snapshot = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("viewport capture succeeds");
    assert_rectangular(&snapshot);
    assert_eq!((snapshot.cols(), snapshot.rows()), (80, 24));
    assert_eq!(snapshot.scope(), SnapshotScope::Viewport);

    // C04: combining marks, wide char with continuation, last-column wide char.
    assert_eq!(snapshot.cell_at(0, 0).expect("cell").text(), "é");
    assert_eq!(snapshot.cell_at(2, 0).expect("cell").text(), "ä");
    assert_eq!(snapshot.cell_at(4, 0).expect("cell").text(), "東");
    assert_eq!(snapshot.cell_at(4, 0).expect("cell").width(), 2);
    assert!(snapshot.cell_at(5, 0).expect("cell").is_continuation());
    let wide = snapshot.cell_at(78, 4).expect("last-column wide char");
    assert_eq!(wide.text(), "東");
    assert_eq!(wide.width(), 2);
    assert!(snapshot.cell_at(79, 4).expect("cell").is_continuation());
    assert_eq!(snapshot.cursor_local(), Some((78, 4)));

    // Bold, italic, and bold-italic are recorded.
    let cells = snapshot.cells();
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "b" && cell.has(Attrs::BOLD) && !cell.has(Attrs::ITALIC)));
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "t" && cell.has(Attrs::ITALIC) && !cell.has(Attrs::BOLD)));
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "i" && cell.has(Attrs::BOLD | Attrs::ITALIC)));

    // SGR 0-15 normalization kept: 31 and 38;5;2 are distinct recorded slots.
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "R" && cell.fg() == SnapshotColor::Indexed(1)));
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "R" && cell.fg() == SnapshotColor::Indexed(2)));
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "R" && cell.fg() == SnapshotColor::Rgb(9, 9, 9)));
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "3" && cell.fg() == SnapshotColor::Rgb(9, 9, 9)));

    // Curly underline with explicit color, reverse video, hyperlink, title.
    let underlined = cells
        .iter()
        .find(|cell| cell.text() == "U")
        .expect("U cell");
    assert_eq!(underlined.underline(), SnapshotUnderline::Curly);
    assert_eq!(underlined.underline_color(), SnapshotColor::Default);
    let colored_underline = cells
        .iter()
        .find(|cell| cell.text() == "V")
        .expect("V cell");
    assert_eq!(colored_underline.underline(), SnapshotUnderline::Curly);
    assert_eq!(
        colored_underline.underline_color(),
        SnapshotColor::Indexed(200)
    );
    assert!(cells
        .iter()
        .any(|cell| cell.text() == "W" && cell.has(Attrs::INVERSE)));
    let linked = cells
        .iter()
        .find(|cell| cell.text() == "L")
        .expect("link cell");
    assert_eq!(linked.link_uri(), Some("https://example.com/live"));
    assert_eq!(linked.link_id(), Some("live"));
    assert_eq!(snapshot.title(), Some("live-title"));

    // C05: palette and default overrides are recorded through Emulator::colors.
    assert_eq!(snapshot.palette()[7], Rgb::new(0x12, 0x34, 0x56));
    assert_eq!(snapshot.default_foreground(), Rgb::new(0x11, 0x11, 0x11));
    assert_eq!(snapshot.default_background(), Rgb::new(0x22, 0x22, 0x22));
    assert_eq!(snapshot.default_cursor(), Rgb::new(0x33, 0x33, 0x33));
    assert!(snapshot.cursor().visible());
    // Blink intent is unknown at this backend: recorded as unknown, not false.
    assert_eq!(snapshot.cursor().blink(), None);
    assert_eq!(snapshot.metadata().source, SnapshotSource::Live);

    // C07: re-observing the same state compares equal (metadata is skipped).
    let again = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("recapture succeeds");
    assert_eq!(snapshot, again);

    // Scopes agree on the shared terminal state.
    let active = session
        .capture_snapshot(SnapshotScope::ActiveScreen)
        .expect("active capture succeeds");
    assert_rectangular(&active);
    assert_eq!(active.title(), Some("live-title"));
    assert_eq!(active.palette()[7], Rgb::new(0x12, 0x34, 0x56));
    let full = session
        .capture_snapshot(SnapshotScope::FullHistory)
        .expect("history capture succeeds");
    assert_rectangular(&full);
    assert_eq!(full.viewport_origin(), 0);
    assert!(full.rows() >= active.rows());

    close(&session);
}

/// U02-B: snapshots stay usable after the session closes (fully owned data).
#[test]
fn u02_b_snapshot_outlives_session_close() {
    let session = session_with_script("b", "owned-data\\r\\n", 80, 24);
    poll_text(&session, "owned-data");
    let snapshot = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("capture succeeds");
    let json_before = serde_json::to_string(&snapshot).expect("serializes");
    close(&session);

    assert!(snapshot.cells().iter().any(|cell| cell.text() == "o"));
    assert_rectangular(&snapshot);
    let json_after = serde_json::to_string(&snapshot).expect("serializes after close");
    assert_eq!(json_before, json_after);
    let back: TerminalSnapshot = serde_json::from_str(&json_after).expect("deserializes");
    assert_eq!(snapshot, back);
}

/// U02-C: locator highlighting never leaks into the default snapshot.
#[test]
fn u02_c_highlighting_needs_explicit_option() {
    let session = session_with_script("c", "findable marker here\\r\\n", 80, 24);
    poll_text(&session, "findable");
    session
        .get_by_text("findable")
        .highlight()
        .expect("highlight applies");

    let raw = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("default capture succeeds");
    let marked = session
        .capture_snapshot_with(&SnapshotOptions::new(SnapshotScope::Viewport).with_highlight(true))
        .expect("highlighted capture succeeds");
    let highlighted_cells: Vec<_> = marked
        .cells()
        .iter()
        .filter(|cell| cell.has(Attrs::INVERSE))
        .collect();
    assert!(!highlighted_cells.is_empty(), "highlight selects cells");
    for (index, cell) in raw.cells().iter().enumerate() {
        if cell.text() == "f" && marked.cells()[index].has(Attrs::INVERSE) {
            assert!(!cell.has(Attrs::INVERSE), "default capture stays raw");
        }
    }
    assert_ne!(raw, marked);
    close(&session);
}

/// U02-D: alternate screen, history coordinates, and resize keep geometry.
#[test]
fn u02_d_scopes_keep_rectangular_geometry() {
    let session = session_with_script("d-alt", "\\033[?1049hALTSCREEN\\r\\n", 80, 24);
    poll_text(&session, "ALTSCREEN");
    let alt = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("alt-screen capture succeeds");
    assert_rectangular(&alt);
    assert_eq!((alt.cols(), alt.rows()), (80, 24));
    assert!(alt.cells().iter().any(|cell| cell.text() == "A"));
    close(&session);

    let mut lines = String::from("\\033[2J\\033[H");
    for line in 0..40 {
        lines.push_str(&format!("history line {line:02}\\r\\n"));
    }
    let session = session_with_script("d-hist", &lines, 80, 24);
    poll_text(&session, "history line 39");

    let viewport = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("viewport capture succeeds");
    let full = session
        .capture_snapshot(SnapshotScope::FullHistory)
        .expect("history capture succeeds");
    assert_rectangular(&viewport);
    assert_rectangular(&full);
    assert!(full.rows() > viewport.rows(), "history extends the grid");
    assert_eq!(full.viewport_origin(), 0);
    // History coordinates use u32, never u16 or a clamped stand-in.
    assert!(full.rows() < 60);
    assert!(u64::from(full.cursor().row()) < full.viewport_origin() + u64::from(full.rows()) + 100);

    // Scroll the viewport up: the cursor stays recorded, outside, unclamped.
    session
        .execute(Operation::Scroll {
            target: ScrollTarget::Lines(5),
        })
        .expect("scroll succeeds");
    let scrolled = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("scrolled capture succeeds");
    assert_rectangular(&scrolled);
    assert!(!scrolled.cursor().in_capture());
    assert_eq!(scrolled.cursor_local(), None);
    assert!(u64::from(scrolled.cursor().row()) >= scrolled.viewport_origin());

    // Resize keeps rectangular geometry at the new dimensions.
    session
        .execute(Operation::Resize {
            cols: 100,
            rows: 30,
        })
        .expect("resize succeeds");
    let resized = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("resized capture succeeds");
    assert_rectangular(&resized);
    assert_eq!((resized.cols(), resized.rows()), (100, 30));
    close(&session);
}

/// U02-E: serialization keeps palette changes and OSC 8 links.
#[test]
fn u02_e_serialization_keeps_palette_and_links() {
    let session = session_with_script(
        "e",
        "\\033]4;5;#654321\\033\\\\\\033]8;id=e2e;https://example.com/kept\\033\\\\KEPT\\033]8;;\\033\\\\\\r\\n",
        80,
        24,
    );
    poll_text(&session, "KEPT");
    let snapshot = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("capture succeeds");
    assert_eq!(snapshot.palette()[5], Rgb::new(0x65, 0x43, 0x21));
    let linked = snapshot
        .cells()
        .iter()
        .find(|cell| cell.text() == "K")
        .expect("link cell");
    assert_eq!(linked.link_uri(), Some("https://example.com/kept"));
    assert_eq!(linked.link_id(), Some("e2e"));

    let json = serde_json::to_string(&snapshot).expect("serializes");
    let back: TerminalSnapshot = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(snapshot, back);
    assert_eq!(back.palette()[5], Rgb::new(0x65, 0x43, 0x21));
    let kept = back
        .cells()
        .iter()
        .find(|cell| cell.text() == "K")
        .expect("link cell");
    assert_eq!(kept.link_uri(), Some("https://example.com/kept"));
    assert_eq!(kept.link_id(), Some("e2e"));
    close(&session);
}

/// U02-F: constructor and JSON reject the same invalid geometry.
#[test]
fn u02_f_constructor_and_json_reject_same_geometry() {
    fn round_trips(parts: &SnapshotParts) -> Result<TerminalSnapshot, String> {
        let snapshot =
            TerminalSnapshot::try_new(parts.clone()).map_err(|error| error.to_string())?;
        let json = serde_json::to_string(&snapshot).expect("serializes");
        serde_json::from_str(&json).map_err(|error| error.to_string())
    }

    // Ragged cell vector.
    let mut parts = blank_parts(2, 2);
    parts.cells.pop();
    assert_eq!(
        TerminalSnapshot::try_new(parts.clone()),
        Err(SnapshotError::CellCountMismatch {
            expected: 4,
            actual: 3
        })
    );
    assert!(round_trips(&parts).is_err());

    // Continuation at column zero.
    let mut parts = blank_parts(2, 1);
    parts.cells[0] = SnapshotCell::new(
        String::new(),
        0,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::empty(),
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("coordinate-free construction succeeds");
    assert_eq!(
        TerminalSnapshot::try_new(parts.clone()),
        Err(SnapshotError::BadContinuation { col: 0, row: 0 })
    );
    assert!(round_trips(&parts).is_err());

    // In-capture cursor outside the grid.
    let mut parts = blank_parts(2, 2);
    parts.cursor = SnapshotCursor::new(
        5,
        5,
        true,
        true,
        SnapshotCursorShape::Block,
        Rgb::new(0, 0, 0),
        None,
    );
    assert_eq!(
        TerminalSnapshot::try_new(parts.clone()),
        Err(SnapshotError::CursorOutOfBounds {
            col: 5,
            row: 5,
            cols: 2,
            rows: 2
        })
    );
    assert!(round_trips(&parts).is_err());
}

/// U02-G: unsupported schemas and oversize input return errors.
#[test]
fn u02_g_schemas_and_limits_are_rejected() {
    let valid = TerminalSnapshot::try_new(blank_parts(2, 2)).expect("valid");
    let mut wire = serde_json::to_value(&valid).expect("serializes");
    wire["schema"] = serde_json::json!(2);
    let error = serde_json::from_value::<TerminalSnapshot>(wire).expect_err("schema 2 fails");
    assert!(
        error.to_string().contains("unsupported snapshot schema"),
        "{error}"
    );

    let mut wire = serde_json::to_value(&valid).expect("serializes");
    wire["cols"] = serde_json::json!(MAX_SNAPSHOT_DIM + 1);
    assert!(serde_json::from_value::<TerminalSnapshot>(wire).is_err());

    // 32768 x 32768 exceeds the cell budget without exceeding a dimension.
    let mut too_many = blank_parts(2, 2);
    too_many.cols = MAX_SNAPSHOT_DIM;
    too_many.rows = MAX_SNAPSHOT_DIM;
    assert_eq!(
        TerminalSnapshot::try_new(too_many),
        Err(SnapshotError::TooManyCells {
            cells: u64::from(MAX_SNAPSHOT_DIM) * u64::from(MAX_SNAPSHOT_DIM),
            limit: MAX_SNAPSHOT_CELLS
        })
    );

    assert!(SnapshotCell::new(
        "x".repeat(MAX_CELL_TEXT_BYTES + 1),
        1,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::empty(),
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .is_err());
}

/// U02-H: capture and serialization keep palette changes at indices 1 and 200.
#[test]
fn u02_h_palette_slots_1_and_200_survive() {
    let session = session_with_script(
        "h",
        "\\033]4;1;#0a0a0a\\033\\\\\\033]4;200;#c8c8c8\\033\\\\READY\\r\\n",
        80,
        24,
    );
    poll_text(&session, "READY");
    let snapshot = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("capture succeeds");
    assert_eq!(snapshot.palette()[1], Rgb::new(0x0a, 0x0a, 0x0a));
    assert_eq!(snapshot.palette()[200], Rgb::new(0xc8, 0xc8, 0xc8));
    let json = serde_json::to_string(&snapshot).expect("serializes");
    let back: TerminalSnapshot = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back.palette()[1], Rgb::new(0x0a, 0x0a, 0x0a));
    assert_eq!(back.palette()[200], Rgb::new(0xc8, 0xc8, 0xc8));
    close(&session);

    let mut parts = blank_parts(1, 1);
    parts.palette[1] = Rgb::new(1, 1, 1);
    parts.palette[200] = Rgb::new(2, 2, 2);
    let offline = TerminalSnapshot::try_new(parts).expect("valid");
    let json = serde_json::to_string(&offline).expect("serializes");
    let back: TerminalSnapshot = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(offline, back);
}

/// U02-I: capture and serialization keep default fg/bg/cursor changes.
#[test]
fn u02_i_default_colors_survive() {
    let session = session_with_script(
        "i",
        "\\033]10;#0b0b0b\\033\\\\\\033]11;#1c1c1c\\033\\\\\\033]12;#2d2d2d\\033\\\\READY\\r\\n",
        80,
        24,
    );
    poll_text(&session, "READY");
    let snapshot = session
        .capture_snapshot(SnapshotScope::Viewport)
        .expect("capture succeeds");
    assert_eq!(snapshot.default_foreground(), Rgb::new(0x0b, 0x0b, 0x0b));
    assert_eq!(snapshot.default_background(), Rgb::new(0x1c, 0x1c, 0x1c));
    assert_eq!(snapshot.default_cursor(), Rgb::new(0x2d, 0x2d, 0x2d));
    let json = serde_json::to_string(&snapshot).expect("serializes");
    let back: TerminalSnapshot = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(snapshot, back);
    close(&session);
}

/// U02-J: equal resolved colors do not erase different color-reference kinds.
#[test]
fn u02_j_default_and_rgb_kinds_stay_distinct() {
    let palette = resolved_palette();
    let mut defaulted = blank_parts(1, 1);
    defaulted.default_foreground = palette[3];
    defaulted.cells[0] = SnapshotCell::new(
        "x".to_string(),
        1,
        SnapshotColor::Default,
        SnapshotColor::Default,
        Attrs::empty(),
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("cell is valid");
    let mut explicit = defaulted.clone();
    explicit.cells[0] = SnapshotCell::new(
        "x".to_string(),
        1,
        SnapshotColor::Rgb(palette[3].r, palette[3].g, palette[3].b),
        SnapshotColor::Default,
        Attrs::empty(),
        SnapshotUnderline::None,
        SnapshotColor::Default,
        None,
        None,
    )
    .expect("cell is valid");
    let a = TerminalSnapshot::try_new(defaulted).expect("valid");
    let b = TerminalSnapshot::try_new(explicit).expect("valid");
    assert_ne!(a, b);
    for snapshot in [&a, &b] {
        let json = serde_json::to_string(snapshot).expect("serializes");
        let back: TerminalSnapshot = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(*snapshot, back);
    }
}

/// U02-K: an unreferenced palette entry still changes canonical state.
#[test]
fn u02_k_unreferenced_palette_change_counts() {
    let left = TerminalSnapshot::try_new(blank_parts(2, 2)).expect("valid");
    let mut parts = blank_parts(2, 2);
    parts.palette[77] = Rgb::new(9, 9, 9);
    let right = TerminalSnapshot::try_new(parts).expect("valid");
    assert_ne!(left, right);
}
