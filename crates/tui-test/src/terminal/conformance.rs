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
//!
//! Assertions here encode the *contract*, not one implementation. Where real
//! emulators legitimately diverge (reflow when shrinking below content width)
//! the test pins only the part that is universal and says why it stops short.

/// Generates the conformance tests for one backend. `$make` builds a boxed
/// emulator from `(cols, rows, &Profile)`.
///
/// The body is fully path-qualified because it expands into the caller's
/// module; it must not collide with whatever that module already imports.
#[macro_export]
macro_rules! emulator_conformance_tests {
    ($make:expr) => {
        fn conformance_emu(
            cols: u16,
            rows: u16,
            scrollback: usize,
        ) -> Box<dyn $crate::terminal::emu::Emulator> {
            conformance_emu_with(
                cols,
                rows,
                $crate::profile::Profile {
                    scrollback,
                    ..Default::default()
                },
            )
        }

        fn conformance_emu_with(
            cols: u16,
            rows: u16,
            profile: $crate::profile::Profile,
        ) -> Box<dyn $crate::terminal::emu::Emulator> {
            let make: fn(
                u16,
                u16,
                &$crate::profile::Profile,
            ) -> Box<dyn $crate::terminal::emu::Emulator> = $make;
            make(cols, rows, &profile)
        }

        /// Row text with trailing blanks removed, for readable assertions.
        fn conformance_text(rows: &[Vec<$crate::terminal::cell::EmuCell>]) -> Vec<String> {
            $crate::terminal::cell::rows_to_strings(rows)
                .into_iter()
                .map(|r| r.trim_end().to_string())
                .collect()
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

        /// `full_rows` is as rectangular as `viewable_rows`; ragged history
        /// rows would misalign the boxed snapshot output.
        #[test]
        fn conformance_full_rows_are_rectangular() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"a\r\nbb\r\nccc\r\ndddd\r\neeeee");
            for row in e.full_rows() {
                assert_eq!(row.len(), 10, "history rows must be full width too");
            }
        }

        /// Printable text lands on the grid, and untouched cells are blank.
        ///
        /// A blank cell must be *fully* default: `expect --bg` and the SVG
        /// renderer read color off cells the shell never painted.
        #[test]
        fn conformance_text_and_blank_cells() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"abc");
            let rows = e.viewable_rows();
            assert_eq!(rows[0][0].ch, "a");
            assert_eq!(rows[0][1].ch, "b");
            assert_eq!(rows[0][2].ch, "c");
            assert_eq!(
                rows[0][3],
                $crate::terminal::cell::EmuCell::default(),
                "an untouched cell must be blank with default colors and no attributes"
            );
            assert_eq!(
                $crate::terminal::cell::rows_to_strings(&rows)[0],
                "abc       "
            );
        }

        /// CR/LF moves to the next row rather than wrapping text together.
        #[test]
        fn conformance_newline_moves_row() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"one\r\ntwo");
            let text = conformance_text(&e.viewable_rows());
            assert_eq!(text[0], "one");
            assert_eq!(text[1], "two");
        }

        /// A bare CR returns to column 0 so later bytes overwrite in place.
        /// Progress bars and readline redraws depend on this.
        #[test]
        fn conformance_carriage_return_overwrites() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"12345\rab");
            assert_eq!(conformance_text(&e.viewable_rows())[0], "ab345");
        }

        /// Backspace moves the cursor back without erasing; the next byte
        /// overwrites in place.
        #[test]
        fn conformance_backspace_moves_cursor() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"abc\x08X");
            assert_eq!(conformance_text(&e.viewable_rows())[0], "abX");
            assert_eq!(e.cursor(), (3, 0));
        }

        /// Text longer than the row wraps onto the next row. `expect text` and
        /// `locator::find` flatten the grid, so wrap placement is observable.
        #[test]
        fn conformance_autowrap_at_right_margin() {
            let mut e = conformance_emu(5, 3, 100);
            e.process(b"abcdefgh");
            let text = conformance_text(&e.viewable_rows());
            assert_eq!(text[0], "abcde", "first row fills to the margin");
            assert_eq!(text[1], "fgh", "the remainder continues on the next row");
        }

        /// A tab advances the cursor to the next 8-column tab stop.
        ///
        /// What a backend leaves *in* the skipped cells is genuinely
        /// divergent — alacritty stores a literal `\t` there, others store
        /// blanks — so this pins only cursor advance and landing column,
        /// which every emulator agrees on.
        #[test]
        fn conformance_tab_advances_to_stop() {
            let mut e = conformance_emu(20, 2, 100);
            e.process(b"a\tb");
            let rows = e.viewable_rows();
            assert_eq!(rows[0][0].ch, "a");
            assert_eq!(rows[0][8].ch, "b", "next glyph lands on the 8-column stop");
            assert_eq!(e.cursor(), (9, 0), "cursor sits just past the tab stop");
        }

        /// Every SGR attribute the cell vocabulary exposes round-trips.
        #[test]
        fn conformance_sgr_attributes() {
            let mut e = conformance_emu(20, 2, 100);
            // bold, dim, italic, underline, inverse, hidden, strikethrough
            e.process(b"\x1b[1;2;3;4;7;8;9mX\x1b[0mY");
            let rows = e.viewable_rows();
            use $crate::terminal::cell::Attrs;
            let set = Attrs::BOLD
                | Attrs::DIM
                | Attrs::ITALIC
                | Attrs::INVERSE
                | Attrs::INVISIBLE
                | Attrs::STRIKE;

            let x = &rows[0][0];
            assert_eq!(x.attrs, set, "every attribute in the SGR must be set");
            assert!(x.underline.is_underlined(), "underline");

            let y = &rows[0][1];
            assert_eq!(y.ch, "Y");
            assert_eq!(y.attrs, Attrs::empty(), "SGR 0 must reset");
            assert_eq!(
                y.underline,
                $crate::terminal::cell::UnderlineStyle::None,
                "SGR 0 must clear underline"
            );
        }

        /// Each extended underline reports its own style, not a flat boolean.
        #[test]
        fn conformance_underline_styles() {
            // Aliased, not glob-imported: `UnderlineStyle::None` would shadow
            // `Option::None` in this scope.
            use $crate::terminal::cell::UnderlineStyle as U;
            for (seq, want) in [
                (&b"\x1b[4m"[..], U::Single),
                (&b"\x1b[4:2m"[..], U::Double),
                (&b"\x1b[4:3m"[..], U::Curly),
                (&b"\x1b[4:4m"[..], U::Dotted),
                (&b"\x1b[4:5m"[..], U::Dashed),
            ] {
                let mut e = conformance_emu(10, 2, 100);
                e.process(seq);
                e.process(b"U");
                let cell = e.viewable_rows()[0][0].clone();
                assert_eq!(cell.underline, want, "{seq:?}");
                assert_eq!(
                    cell.underline_color, None,
                    "no SGR 58 means it follows the fg"
                );
            }
        }

        /// SGR 58 colors the underline independently of the text.
        #[test]
        fn conformance_underline_color() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b[31;4;58;5;33mU");
            let cell = e.viewable_rows()[0][0].clone();
            assert_eq!(cell.fg, Some($crate::terminal::cell::Color::from_index(1)));
            assert_eq!(
                cell.underline_color,
                Some($crate::terminal::cell::Color::from_index(33)),
                "underline keeps its own color"
            );
        }

        /// The underline's color is tracked separately from its shape, so
        /// SGR 58 survives a cell that is not underlined and SGR 24 leaves it
        /// alone. Only a full reset clears both.
        #[test]
        fn conformance_underline_color_outlives_the_underline() {
            use $crate::terminal::cell::{Color, UnderlineStyle as U};
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b[58;5;33mA\x1b[4mB\x1b[24mC\x1b[0mD");
            let rows = e.viewable_rows();

            assert_eq!(rows[0][0].underline, U::None, "58 alone does not underline");
            assert_eq!(
                rows[0][0].underline_color,
                Some(Color::from_index(33)),
                "but its color is still tracked"
            );
            assert_eq!(rows[0][1].underline, U::Single, "4 turns it on");
            assert_eq!(rows[0][1].underline_color, Some(Color::from_index(33)));
            assert_eq!(rows[0][2].underline, U::None, "24 turns it off");
            assert_eq!(
                rows[0][2].underline_color,
                Some(Color::from_index(33)),
                "24 clears the shape, not the color"
            );
            assert_eq!(rows[0][3].underline, U::None, "0 resets everything");
            assert_eq!(rows[0][3].underline_color, None);
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
            assert_eq!(
                rows[0][0].fg,
                Some($crate::terminal::cell::Color::Named(
                    $crate::terminal::cell::NamedColor::Red
                )),
                "SGR 31 is the themeable red slot, not a fixed index"
            );
            assert_eq!(
                rows[0][1].fg,
                Some($crate::terminal::cell::Color::Idx(196)),
                "256-color palette index stays an index"
            );
            assert_eq!(
                rows[0][2].fg,
                Some($crate::terminal::cell::Color::Rgb(10, 20, 30)),
                "24-bit truecolor"
            );
            assert_eq!(rows[0][3].fg, None, "reset returns to the terminal default");
        }

        /// Background colors are tracked independently of foreground.
        #[test]
        fn conformance_background_color() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b[44mB");
            let cell = e.viewable_rows()[0][0].clone();
            assert_eq!(
                cell.bg,
                Some($crate::terminal::cell::Color::Named(
                    $crate::terminal::cell::NamedColor::Blue
                )),
                "named blue background"
            );
            assert_eq!(cell.fg, None);
        }

        /// A double-width char owns both its columns: the second holds the
        /// continuation marker, distinct from a blank cell's space, so text
        /// extraction does not invent a column that the terminal never drew.
        #[test]
        fn conformance_wide_char_continuation() {
            let mut e = conformance_emu(10, 2, 100);
            e.process("你a".as_bytes());
            let rows = e.viewable_rows();
            assert_eq!(rows[0][0].ch, "你");
            assert_eq!(
                rows[0][1].ch,
                $crate::terminal::cell::CONTINUATION,
                "the second column of a wide char is a continuation, not a blank"
            );
            assert_eq!(rows[0][2].ch, "a", "next char sits after the continuation");
            assert_eq!(rows[0][3].ch, " ", "an untouched cell is a blank space");
            assert_eq!(
                $crate::terminal::cell::rows_to_strings(&rows)[0],
                "你a       ",
                "the row is 10 columns wide, not 11"
            );
        }

        /// A wide char that does not fit in the last column wraps whole to the
        /// next row, and the column it left behind is a blank it still owns.
        /// Backends mark that filler with a distinct flag from a real
        /// continuation; conflating the two loses a column and every row after
        /// the wrap renders shifted.
        #[test]
        fn conformance_wide_char_wraps_at_the_line_edge() {
            let mut e = conformance_emu(5, 3, 100);
            e.process("abcd你".as_bytes());
            let rows = e.viewable_rows();
            assert_eq!(
                rows[0][4].ch, " ",
                "the column the wide char vacated is a blank"
            );
            assert_eq!(
                rows[1][0].ch, "你",
                "the wide char moved to the next row whole"
            );
            assert_eq!(
                rows[1][1].ch,
                $crate::terminal::cell::CONTINUATION,
                "and takes its continuation with it"
            );
            let text = $crate::terminal::cell::rows_to_strings(&rows);
            assert_eq!(text[0], "abcd ", "the padded row is still 5 columns wide");
            assert_eq!(text[1], "你   ", "the wrapped row is 5 columns wide too");
        }

        /// The snapshot serializer, run against a real backend grid rather
        /// than hand-built rows. This is the pairing that broke in practice:
        /// the grid was right and `serialize` was right on its own inputs, but
        /// the frame still came out misaligned because they disagreed on what
        /// a continuation renders as. Any backend whose wrapping is off by a
        /// column shows up here as a row that overruns the box.
        #[test]
        fn conformance_snapshot_frames_a_wrapped_wide_char() {
            let mut e = conformance_emu(5, 3, 100);
            e.process("abcd你".as_bytes());
            assert_eq!(
                $crate::assert::snapshot::serialize(&e.viewable_rows(), 5, false, None),
                concat!(
                    "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n",
                    "\u{2502}abcd \u{2502}\n",
                    "\u{2502}\u{4f60}   \u{2502}\n",
                    "\u{2502}     \u{2502}\n",
                    "\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}",
                )
            );
        }

        /// The whole grid after text wraps over several rows, wide char
        /// included. Pinning every row at once catches the off-by-one column
        /// errors a per-cell assertion reads past: here the wide char fits
        /// mid-row, so no padding is involved and every row is 5 columns.
        #[test]
        fn conformance_wrap_grid_over_several_rows() {
            let mut e = conformance_emu(5, 3, 100);
            e.process("abcdefg你ij".as_bytes());
            assert_eq!(
                $crate::terminal::cell::rows_to_strings(&e.viewable_rows()),
                // "fg你i" is 4 chars but 5 columns: 你 spans two.
                ["abcde", "fg你i", "j    "]
            );
        }

        /// A multi-byte character split across reads must still decode. The
        /// reader fills a fixed 8 KiB buffer, so this happens on real output;
        /// a backend that decoded each chunk independently would corrupt the
        /// grid here while passing every other case.
        #[test]
        fn conformance_split_utf8_sequence() {
            let mut e = conformance_emu(10, 2, 100);
            let text = "héllo".as_bytes();
            e.process(&text[..2]); // splits the two-byte 'é'
            e.process(&text[2..]);
            assert_eq!(conformance_text(&e.viewable_rows())[0], "héllo");
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

        /// Positioning past the edge clamps to the last cell, rather than
        /// wrapping, saturating to zero, or escaping the grid.
        #[test]
        fn conformance_cursor_clamped_to_grid() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"\x1b[999;999H");
            assert_eq!(
                e.cursor(),
                (9, 3),
                "out-of-range CUP clamps to the bottom-right cell"
            );
        }

        /// Growing the terminal updates the reported size and keeps content.
        #[test]
        fn conformance_resize_grow_preserves_content() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"hello");
            e.resize(20, 6);

            assert_eq!(e.size(), (20, 6));
            let rows = e.viewable_rows();
            assert_eq!(rows.len(), 6);
            assert_eq!(rows[0].len(), 20);
            assert_eq!(
                conformance_text(&rows)[0],
                "hello",
                "growing must not discard existing content"
            );
        }

        /// Shrinking reports the new size and keeps content that still fits.
        ///
        /// Reflow of content wider than the new width legitimately differs
        /// between emulators, so this pins only the narrow, universal case.
        #[test]
        fn conformance_resize_shrink_keeps_fitting_content() {
            let mut e = conformance_emu(20, 6, 100);
            e.process(b"hey");
            e.resize(10, 4);

            assert_eq!(e.size(), (10, 4));
            let rows = e.viewable_rows();
            assert_eq!(rows.len(), 4);
            assert_eq!(rows[0].len(), 10);
            assert!(
                conformance_text(&rows).iter().any(|r| r == "hey"),
                "content narrower than the new width must survive shrinking"
            );
        }

        /// Scrolled-off lines leave the viewport but stay in `full_rows`, and
        /// `full_rows` ends with exactly the viewport. `grid(full = true)` in
        /// the daemon depends on that ordering.
        #[test]
        fn conformance_scrollback_retained_and_ordered() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"L1\r\nL2\r\nL3\r\nL4\r\nL5\r\nL6");

            let view = e.viewable_rows();
            let view_text = conformance_text(&view);
            assert_eq!(view_text.len(), 3);
            assert_eq!(view_text[2], "L6", "viewport shows the newest line");
            assert!(
                !view_text.iter().any(|r| r == "L1"),
                "L1 must have scrolled out of the viewport"
            );

            let full = e.full_rows();
            assert!(
                full.len() > view.len(),
                "full_rows must include scrollback: {} vs {}",
                full.len(),
                view.len()
            );
            assert!(
                conformance_text(&full).iter().any(|r| r == "L1"),
                "scrolled-off L1 must survive in full_rows"
            );
            assert_eq!(
                &full[full.len() - view.len()..],
                &view[..],
                "full_rows must be history followed by exactly the viewport"
            );
        }

        /// With no scrolling, history is empty and `full_rows` *is* the
        /// viewport.
        #[test]
        fn conformance_full_rows_without_history() {
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"only");
            assert_eq!(
                e.full_rows(),
                e.viewable_rows(),
                "no scroll means full_rows matches the viewport exactly"
            );
        }

        /// The scrollback limit is honored. A backend that ignores it grows
        /// without bound in a long-lived daemon session.
        #[test]
        fn conformance_scrollback_is_bounded() {
            let rows = 3u16;
            let scrollback = 2usize;
            let mut e = conformance_emu(10, rows, scrollback);
            for i in 0..20 {
                e.process(format!("line{i}\r\n").as_bytes());
            }
            let total = e.full_rows().len();
            assert!(
                total >= rows as usize,
                "full_rows ({total}) must still contain the viewport"
            );
            assert!(
                total <= rows as usize + scrollback + 1,
                "full_rows ({total}) must respect the {scrollback}-row scrollback limit"
            );
        }

        /// Queries that require an answer are queued for the PTY, and draining
        /// is destructive so replies are not sent twice. The reply must be
        /// available as soon as `process` returns: a backend that parsed
        /// asynchronously would hang every program that probes the terminal.
        #[test]
        fn conformance_pty_write_back() {
            let mut e = conformance_emu(10, 4, 100);
            // A backend may queue its own startup handshake; only the reply to
            // our query matters.
            let _ = e.take_pending_writes();

            // Device Status Report: the terminal must answer with the cursor
            // position, 1-based, as CSI <row> ; <col> R.
            e.process(b"\x1b[3;5H\x1b[6n");
            let reply = e.take_pending_writes();
            assert_eq!(
                String::from_utf8_lossy(&reply),
                "\x1b[3;5R",
                "DSR must report the cursor position, or programs hang waiting"
            );
            assert!(
                e.take_pending_writes().is_empty(),
                "draining must consume the queue"
            );
        }

        /// A color query is answered with the session's configured color.
        ///
        /// Programs query the background to decide whether they are on a light
        /// or a dark terminal. A backend that stays silent leaves them blocked
        /// until they time out and guess.
        #[test]
        fn conformance_color_queries_are_answered() {
            use $crate::profile::{Colors, Profile, Rgb};
            let profile = Profile {
                colors: Colors {
                    background: Rgb::new(0x12, 0x34, 0x56),
                    red: Rgb::new(0xab, 0xcd, 0xef),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut e = conformance_emu_with(10, 4, profile);
            let _ = e.take_pending_writes();

            e.process(b"\x1b]11;?\x07");
            assert_eq!(
                String::from_utf8_lossy(&e.take_pending_writes()),
                "\x1b]11;rgb:1212/3434/5656\x07",
                "OSC 11 must report the configured background"
            );

            e.process(b"\x1b]4;1;?\x07");
            assert_eq!(
                String::from_utf8_lossy(&e.take_pending_writes()),
                "\x1b]4;1;rgb:abab/cdcd/efef\x07",
                "OSC 4 must report the configured palette entry"
            );
        }

        /// A reply uses the terminator the query used. A program that reads
        /// until the terminator it sent would otherwise wait for one that
        /// never comes.
        #[test]
        fn conformance_a_color_reply_echoes_the_terminator() {
            let mut e = conformance_emu(10, 4, 100);
            let _ = e.take_pending_writes();

            e.process(b"\x1b]11;?\x07");
            let bel = e.take_pending_writes();
            assert!(
                bel.ends_with(b"\x07"),
                "a BEL query is answered with BEL: {:?}",
                String::from_utf8_lossy(&bel)
            );

            e.process(b"\x1b]11;?\x1b\\");
            let st = e.take_pending_writes();
            assert!(
                st.ends_with(b"\x1b\\"),
                "an ST query is answered with ST: {:?}",
                String::from_utf8_lossy(&st)
            );
        }

        /// Replies leave in the order the program asked for them.
        ///
        /// A batch of queries is commonly ended with a device attributes
        /// request, whose reply every terminal sends, and the reply to it is
        /// read as the end of the batch. An answer that arrived after it would
        /// look like the query went unanswered, and would then be read as
        /// though the user had typed it.
        #[test]
        fn conformance_replies_keep_the_order_they_were_asked_in() {
            let mut e = conformance_emu(10, 4, 100);
            let _ = e.take_pending_writes();

            e.process(b"\x1b]11;?\x07\x1b[c");
            let asked_color_first = e.take_pending_writes();
            let color = asked_color_first
                .windows(2)
                .position(|w| w == b"]1")
                .expect("a color reply");
            let attributes = asked_color_first
                .windows(2)
                .position(|w| w == b"[?")
                .expect("a device attributes reply");
            assert!(
                color < attributes,
                "the color was asked for first, so it is answered first: {:?}",
                String::from_utf8_lossy(&asked_color_first)
            );

            e.process(b"\x1b[c\x1b]11;?\x07");
            let asked_color_second = e.take_pending_writes();
            let color = asked_color_second
                .windows(2)
                .position(|w| w == b"]1")
                .expect("a color reply");
            let attributes = asked_color_second
                .windows(2)
                .position(|w| w == b"[?")
                .expect("a device attributes reply");
            assert!(
                attributes < color,
                "and asked for second, it is answered second: {:?}",
                String::from_utf8_lossy(&asked_color_second)
            );
        }

        /// A program can shadow a color, and a reset puts the configured one
        /// back. The configured color is never reachable, so a reset always
        /// has something to restore.
        #[test]
        fn conformance_a_color_set_is_undone_by_a_reset() {
            use $crate::profile::{Colors, Profile, Rgb};
            let configured = Rgb::new(0x11, 0x22, 0x33);
            let profile = Profile {
                colors: Colors {
                    background: configured,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut e = conformance_emu_with(10, 4, profile);
            let background = $crate::profile::ColorSlot::Background;
            assert_eq!(e.color(background), configured);

            e.process(b"\x1b]11;#654321\x07");
            assert_eq!(
                e.color(background),
                Rgb::new(0x65, 0x43, 0x21),
                "a set shadows the configured color"
            );

            e.process(b"\x1b]111\x07");
            assert_eq!(
                e.color(background),
                configured,
                "OSC 111 restores the configured color"
            );

            // The same for a palette entry, which resets with OSC 104.
            e.process(b"\x1b]4;2;#010203\x07");
            assert_eq!(
                e.color($crate::profile::ColorSlot::Indexed(2)),
                Rgb::new(1, 2, 3)
            );
            e.process(b"\x1b]104;2\x07");
            assert_eq!(
                e.color($crate::profile::ColorSlot::Indexed(2)),
                Colors::default().green
            );
        }

        /// Each dynamic color is addressed by its own sequence, and each is
        /// reset by its own.
        ///
        /// `OSC 11` is the one programs reach for, so it is easy to wire that
        /// up and leave the foreground or the cursor answering the wrong slot.
        #[test]
        fn conformance_each_dynamic_color_is_separately_addressable() {
            use $crate::profile::ColorSlot;
            let mut e = conformance_emu(10, 4, 100);
            let before = [
                e.color(ColorSlot::Foreground),
                e.color(ColorSlot::Background),
                e.color(ColorSlot::Cursor),
            ];

            // Set all three to distinct colors, then check none bled into
            // another.
            e.process(b"\x1b]10;#111111\x07\x1b]11;#222222\x07\x1b]12;#333333\x07");
            assert_eq!(e.color(ColorSlot::Foreground), Rgb::new(0x11, 0x11, 0x11));
            assert_eq!(e.color(ColorSlot::Background), Rgb::new(0x22, 0x22, 0x22));
            assert_eq!(e.color(ColorSlot::Cursor), Rgb::new(0x33, 0x33, 0x33));

            // And each reset frees only its own slot.
            e.process(b"\x1b]110\x07");
            assert_eq!(e.color(ColorSlot::Foreground), before[0], "110 resets fg");
            assert_eq!(
                e.color(ColorSlot::Background),
                Rgb::new(0x22, 0x22, 0x22),
                "110 must leave the background alone"
            );

            e.process(b"\x1b]112\x07");
            assert_eq!(
                e.color(ColorSlot::Cursor),
                before[2],
                "112 resets the cursor"
            );
            assert_eq!(
                e.color(ColorSlot::Background),
                Rgb::new(0x22, 0x22, 0x22),
                "112 must leave the background alone"
            );

            e.process(b"\x1b]111\x07");
            assert_eq!(e.color(ColorSlot::Background), before[1], "111 resets bg");
        }

        /// Every dynamic color answers a query, not just the background.
        #[test]
        fn conformance_every_dynamic_color_answers_a_query() {
            let mut e = conformance_emu(10, 4, 100);
            let _ = e.take_pending_writes();

            e.process(b"\x1b]10;#010203\x07\x1b]11;#040506\x07\x1b]12;#070809\x07");
            let _ = e.take_pending_writes();

            for (query, expected) in [
                (&b"\x1b]10;?\x07"[..], "\x1b]10;rgb:0101/0202/0303\x07"),
                (b"\x1b]11;?\x07", "\x1b]11;rgb:0404/0505/0606\x07"),
                (b"\x1b]12;?\x07", "\x1b]12;rgb:0707/0808/0909\x07"),
            ] {
                e.process(query);
                assert_eq!(
                    String::from_utf8_lossy(&e.take_pending_writes()),
                    expected,
                    "querying {:?}",
                    String::from_utf8_lossy(query)
                );
            }
        }

        /// `OSC 104` with no index resets the whole palette, and leaves the
        /// three dynamic colors alone: they have their own resets.
        #[test]
        fn conformance_a_bare_palette_reset_spares_the_dynamic_colors() {
            use $crate::profile::ColorSlot;
            let mut e = conformance_emu(10, 4, 100);
            let configured_red = e.color(ColorSlot::Indexed(1));

            e.process(b"\x1b]4;1;#111111;200;#222222\x07\x1b]11;#333333\x07");
            assert_eq!(e.color(ColorSlot::Indexed(1)), Rgb::new(0x11, 0x11, 0x11));
            assert_eq!(e.color(ColorSlot::Indexed(200)), Rgb::new(0x22, 0x22, 0x22));

            e.process(b"\x1b]104\x07");
            assert_eq!(e.color(ColorSlot::Indexed(1)), configured_red);
            assert_eq!(
                e.color(ColorSlot::Background),
                Rgb::new(0x33, 0x33, 0x33),
                "a palette reset is not a background reset"
            );
        }

        /// An unconfigured palette entry still answers, from the table the
        /// specification defines for it.
        #[test]
        fn conformance_an_unconfigured_index_resolves_from_the_spec_table() {
            use $crate::profile::Rgb;
            let e = conformance_emu(10, 4, 100);
            assert_eq!(
                e.color($crate::profile::ColorSlot::Indexed(196)),
                Rgb::new(255, 0, 0),
                "index 196 is pure red in the xterm color cube"
            );
            assert_eq!(
                e.color($crate::profile::ColorSlot::Indexed(232)),
                Rgb::new(8, 8, 8),
                "the gray ramp starts at 8"
            );
        }

        /// A cell records which slot it chose, never a color, so what it
        /// paints follows whatever that slot currently holds.
        #[test]
        fn conformance_a_cell_follows_its_slot() {
            use $crate::profile::Rgb;
            let mut e = conformance_emu(10, 4, 100);
            e.process(b"\x1b[31mR");
            let cell = e.viewable_rows()[0][0].clone();

            let before = e.resolve(cell.fg, true);
            e.process(b"\x1b]4;1;#0a0b0c\x07");
            assert_eq!(
                e.resolve(cell.fg, true),
                Rgb::new(0x0a, 0x0b, 0x0c),
                "recoloring the slot recolors the cell that chose it"
            );
            assert_ne!(before, e.resolve(cell.fg, true));
        }

        /// The alternate screen hides primary content and restores it on exit.
        #[test]
        fn conformance_alt_screen_round_trip() {
            let mut e = conformance_emu(10, 3, 100);
            e.process(b"primary");
            e.process(b"\x1b[?1049h");
            let alt = conformance_text(&e.viewable_rows());
            assert!(
                !alt.iter().any(|r| r.contains("primary")),
                "alt screen must start clear"
            );

            e.process(b"\x1b[?1049l");
            let back = conformance_text(&e.viewable_rows());
            assert!(
                back.iter().any(|r| r.contains("primary")),
                "leaving alt screen restores primary content"
            );
        }

        /// Erase resets cells to fully default, not merely to a space.
        #[test]
        fn conformance_erase_clears_cells() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"abcdef");
            e.process(b"\x1b[H\x1b[2J");
            for (x, cell) in e.viewable_rows()[0].iter().enumerate() {
                assert_eq!(
                    cell,
                    &$crate::terminal::cell::EmuCell::default(),
                    "ED 2 must reset cell {x} to a default cell"
                );
            }
        }

        /// Erase to end of line clears from the cursor rightward only.
        #[test]
        fn conformance_erase_line_from_cursor() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"abcdef");
            e.process(b"\x1b[1;4H\x1b[K");
            assert_eq!(
                conformance_text(&e.viewable_rows())[0],
                "abc",
                "EL clears from the cursor to end of line, keeping the prefix"
            );
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
                Some($crate::terminal::cell::Color::from_index(1)),
                "a sequence split across process() calls must still apply"
            );
        }

        /// `OSC 0` and `OSC 2` both set the window title, and either
        /// terminator ends them. A backend that only accepted one spelling
        /// would miss the title from whichever programs use the other.
        #[test]
        fn conformance_title_is_set_by_osc_0_and_2() {
            let mut e = conformance_emu(10, 2, 100);
            assert_eq!(e.title(), None, "a fresh terminal has no title");

            e.process(b"\x1b]2;from osc 2\x07");
            assert_eq!(e.title().as_deref(), Some("from osc 2"));

            e.process(b"\x1b]0;from osc 0\x07");
            assert_eq!(
                e.title().as_deref(),
                Some("from osc 0"),
                "osc 0 sets the same title as osc 2"
            );

            e.process(b"\x1b]2;st terminated\x1b\\");
            assert_eq!(
                e.title().as_deref(),
                Some("st terminated"),
                "ST ends the sequence just as BEL does"
            );
        }

        /// An empty title is a request for no title, not for a blank one.
        ///
        /// Programs clear the title this way on exit, so reporting `Some("")`
        /// would leave a caller unable to tell a cleared title from one that
        /// was never set without checking for a special case.
        #[test]
        fn conformance_an_empty_title_resets() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b]2;something\x07");
            assert!(e.title().is_some());

            e.process(b"\x1b]2;\x07");
            assert_eq!(e.title(), None, "an empty title clears it");

            e.process(b"\x1b]0;again\x07");
            e.process(b"\x1b]0;\x07");
            assert_eq!(e.title(), None, "and osc 0 clears it too");
        }

        /// The title survives output and is not tied to the grid: clearing the
        /// screen is not clearing the title.
        #[test]
        fn conformance_title_outlives_screen_content() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b]2;kept\x07");
            e.process(b"text\r\n\x1b[2J");
            assert_eq!(
                e.title().as_deref(),
                Some("kept"),
                "erasing the screen leaves the title alone"
            );
        }

        /// The title stack (`CSI 22 t` pushes, `CSI 23 t` pops) lets a program
        /// set a title and put back whatever was there before, which is how a
        /// shell restores the title after running a command.
        #[test]
        fn conformance_title_stack_pushes_and_pops() {
            let mut e = conformance_emu(10, 2, 100);
            e.process(b"\x1b]2;shell\x07");
            e.process(b"\x1b[22t");
            e.process(b"\x1b]2;running a command\x07");
            assert_eq!(e.title().as_deref(), Some("running a command"));

            e.process(b"\x1b[23t");
            assert_eq!(
                e.title().as_deref(),
                Some("shell"),
                "popping restores the pushed title"
            );
        }
    };
}
