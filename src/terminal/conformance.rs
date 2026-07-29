//! Backend-agnostic conformance suite for [`crate::terminal::emu::Emulator`].
//!
//! Every backend must produce the same grid for the same bytes, otherwise
//! swapping emulators silently changes what `expect`, `snapshot`, and the SVG
//! renderer see. Backends opt in with one line:
//!
//! ```ignore
//! crate::emulator_conformance_tests!(|c, r, s| Box::new(AlacrittyEmu::new(c, r, s)));
//! ```
//!
//! Each case becomes a separate `#[test]` in the calling module, so a failure
//! names the exact part of the contract that broke.

/// Generates the conformance tests for one backend. `$make` builds a boxed
/// emulator from `(cols, rows, scrollback)`.
#[macro_export]
macro_rules! emulator_conformance_tests {
    ($make:expr) => {
        #[allow(unused_imports)]
        use $crate::terminal::cell::{rows_to_strings, Color};
        #[allow(unused_imports)]
        use $crate::terminal::emu::Emulator;

        fn conformance_emu(cols: u16, rows: u16, scrollback: usize) -> Box<dyn Emulator> {
            let make: fn(u16, u16, usize) -> Box<dyn Emulator> = $make;
            make(cols, rows, scrollback)
        }

        /// The grid is always exactly `rows` x `cols`, regardless of content.
        #[test]
        fn conformance_grid_shape_is_exact() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"hi");
            let rows = e.viewable_rows();
            assert_eq!(rows.len(), 4, "row count must equal terminal rows");
            for row in &rows {
                assert_eq!(row.len(), 10, "every row must be full width");
            }
            assert_eq!(e.size(), (10, 4));
        }

        /// Printable text lands on the grid, and untouched cells are blank.
        #[test]
        fn conformance_text_and_blank_cells() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"abc");
            let rows = e.viewable_rows();
            assert_eq!(rows[0][0].ch, "a");
            assert_eq!(rows[0][1].ch, "b");
            assert_eq!(rows[0][2].ch, "c");
            assert_eq!(
                rows[0][3].ch, "",
                "blank cells must be the empty string, not a space"
            );
            assert_eq!(rows_to_strings(&rows)[0], "abc       ");
        }

        /// CR/LF moves to the next row rather than wrapping text together.
        #[test]
        fn conformance_newline_moves_row() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"one\r\ntwo");
            let text = rows_to_strings(&e.viewable_rows());
            assert_eq!(text[0].trim_end(), "one");
            assert_eq!(text[1].trim_end(), "two");
        }

        /// Every SGR attribute the cell vocabulary exposes round-trips.
        #[test]
        fn conformance_sgr_attributes() {
            let mut e = conformance_emu(20, 2, 100);
            // bold, dim, italic, underline, inverse, hidden, strikethrough
            e.process(b"\x1b[1;2;3;4;7;8;9mX\x1b[0mY");
            let rows = e.viewable_rows();
            let x = &rows[0][0];
            assert!(x.bold, "bold");
            assert!(x.dim, "dim");
            assert!(x.italic, "italic");
            assert!(x.underline, "underline");
            assert!(x.inverse, "inverse");
            assert!(x.invisible, "invisible");
            assert!(x.strike, "strike");

            let y = &rows[0][1];
            assert!(!y.bold && !y.dim && !y.italic, "SGR 0 must reset");
            assert!(!y.underline && !y.inverse && !y.invisible && !y.strike);
        }

        /// Extended underline styles still report as `underline`.
        #[test]
        fn conformance_extended_underline_is_underline() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b[4:3mU");
            assert!(
                e.viewable_rows()[0][0].underline,
                "curly underline must map to underline"
            );
        }

        /// Named, 256-palette, and 24-bit colors map onto the color vocabulary.
        #[test]
        fn conformance_colors() {
            let mut e = conformance_emu(20, 2, 100);
            e.process(b"\x1b[31mR");
            e.process(b"\x1b[38;5;196mP");
            e.process(b"\x1b[38;2;10;20;30mT");
            e.process(b"\x1b[0mD");
            let rows = e.viewable_rows();
            assert_eq!(rows[0][0].fg, Color::Idx(1), "named red is palette index 1");
            assert_eq!(rows[0][1].fg, Color::Idx(196), "256-color palette index");
            assert_eq!(rows[0][2].fg, Color::Rgb(10, 20, 30), "24-bit truecolor");
            assert_eq!(
                rows[0][3].fg,
                Color::Default,
                "reset returns to default, not an index"
            );
        }

        /// Background colors are tracked independently of foreground.
        #[test]
        fn conformance_background_color() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b[44mB");
            let cell = e.viewable_rows()[0][0].clone();
            assert_eq!(cell.bg, Color::Idx(4), "named blue background");
            assert_eq!(cell.fg, Color::Default);
        }

        /// A double-width char occupies its cell; its spacer reads as blank so
        /// text extraction keeps column alignment.
        #[test]
        fn conformance_wide_char_spacer_is_blank() {
            let mut e = conformance_emu(10, 2, 100);
            e.process("你a".as_bytes());
            let rows = e.viewable_rows();
            assert_eq!(rows[0][0].ch, "你");
            assert_eq!(rows[0][1].ch, "", "wide-char spacer must be blank");
            assert_eq!(rows[0][2].ch, "a", "next char sits after the spacer");
        }

        /// Cursor tracks writes and absolute positioning, 0-based as (x, y).
        #[test]
        fn conformance_cursor_position() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"abc");
            assert_eq!(e.cursor(), (3, 0), "cursor follows printed text");

            e.process(b"\x1b[3;5H");
            assert_eq!(e.cursor(), (4, 2), "CUP is 1-based, cursor() is 0-based");
        }

        /// The cursor never escapes the grid, even when driven past the edge.
        #[test]
        fn conformance_cursor_clamped_to_grid() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"\x1b[999;999H");
            let (x, y) = e.cursor();
            assert!(x < 10, "cursor x {x} must stay inside 10 cols");
            assert!(y < 4, "cursor y {y} must stay inside 4 rows");
        }

        /// Resizing updates the reported size and the grid shape.
        #[test]
        fn conformance_resize() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"hello");
            e.resize(20, 6);
            assert_eq!(e.size(), (20, 6));
            let rows = e.viewable_rows();
            assert_eq!(rows.len(), 6);
            assert_eq!(rows[0].len(), 20);
        }

        /// Scrolled-off lines leave the viewport but stay in `full_rows`.
        #[test]
        fn conformance_scrollback_retained() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"L1\r\nL2\r\nL3\r\nL4\r\nL5\r\nL6");

            let view = rows_to_strings(&e.viewable_rows());
            assert_eq!(view.len(), 3);
            assert_eq!(view[2].trim_end(), "L6", "viewport shows the newest line");
            assert!(
                !view.iter().any(|r| r.trim_end() == "L1"),
                "L1 must have scrolled out of the viewport"
            );

            let full = rows_to_strings(&e.full_rows());
            assert!(
                full.len() > view.len(),
                "full_rows must include scrollback: {} vs {}",
                full.len(),
                view.len()
            );
            assert!(
                full.iter().any(|r| r.trim_end() == "L1"),
                "scrolled-off L1 must survive in full_rows"
            );
            assert_eq!(
                full.last().map(|r| r.trim_end().to_string()),
                Some("L6".to_string()),
                "full_rows ends with the newest line (history first, screen last)"
            );
        }

        /// With no scrolling, history is empty and both views agree.
        #[test]
        fn conformance_full_rows_without_history() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"only");
            assert_eq!(
                e.full_rows().len(),
                e.viewable_rows().len(),
                "no scroll means no extra history rows"
            );
        }

        /// Queries that require an answer are queued for the PTY, and draining
        /// is destructive so replies are not sent twice.
        #[test]
        fn conformance_pty_write_back() {
            let mut e = conformance_emu(10, 4, 100);
            assert!(
                e.take_pending_writes().is_empty(),
                "nothing pending before any query"
            );

            // Device Status Report: the terminal must answer with a position.
            e.process(b"\x1b[6n");
            let reply = e.take_pending_writes();
            assert!(
                !reply.is_empty(),
                "DSR must produce a reply, or programs will hang waiting"
            );
            assert!(
                reply.starts_with(b"\x1b["),
                "reply should be a CSI sequence, got {:?}",
                String::from_utf8_lossy(&reply)
            );
            assert!(
                e.take_pending_writes().is_empty(),
                "draining must consume the queue"
            );
        }

        /// The alternate screen hides primary content and restores it on exit.
        #[test]
        fn conformance_alt_screen_round_trip() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"primary");
            e.process(b"\x1b[?1049h");
            let alt = rows_to_strings(&e.viewable_rows());
            assert!(
                !alt.iter().any(|r| r.contains("primary")),
                "alt screen must start clear"
            );

            e.process(b"\x1b[?1049l");
            let back = rows_to_strings(&e.viewable_rows());
            assert!(
                back.iter().any(|r| r.contains("primary")),
                "leaving alt screen restores primary content"
            );
        }

        /// Erase sequences clear cells back to blank.
        #[test]
        fn conformance_erase_clears_cells() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"abcdef");
            e.process(b"\x1b[H\x1b[2J");
            let text = rows_to_strings(&e.viewable_rows());
            assert_eq!(text[0], "          ", "ED 2 clears the screen");
        }

        /// Byte-split escape sequences still parse; PTY reads chunk arbitrarily.
        #[test]
        fn conformance_split_escape_sequence() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b[");
            e.process(b"31m");
            e.process(b"R");
            assert_eq!(
                e.viewable_rows()[0][0].fg,
                Color::Idx(1),
                "a sequence split across process() calls must still apply"
            );
        }
    };
}
