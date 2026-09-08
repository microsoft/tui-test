use super::ansi;
use crate::engine::LiveFrame as Frame;
use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle};
use crate::terminal::emu::{KeyboardMode, MouseMode};

/// The target's input modes as last announced to the viewer's terminal, so a
/// mode is only re-applied when the target changes it.
#[derive(Default)]
pub struct ModeMirror {
    applied: Option<(KeyboardMode, bool, MouseMode)>,
}

/// Space inside the shared one-cell monitor border.
pub fn content_size(viewer: (u16, u16)) -> (u16, u16) {
    (viewer.0.max(8) - 2, viewer.1.max(4) - 2)
}

/// Render a framed, full-color view of `frame` clipped to the `viewer` size.
///
/// `None` renders a "no active session" placeholder. The output positions
/// itself from the home cell and clears trailing cells/rows, so successive
/// frames repaint in place without flicker (no full screen clear).
pub fn render_frame(
    frame: Option<&Frame>,
    viewer: (u16, u16),
    session: &str,
    interactive: bool,
    modes: &mut ModeMirror,
) -> String {
    let available = content_size(viewer);
    let inner_w = match frame {
        Some(f) => f.size.0.min(available.0),
        None => available.0,
    } as usize;
    let inner_h = match frame {
        Some(f) => f.size.1.min(available.1),
        None => available.1,
    } as usize;
    let too_small = viewer.0 < 8
        || viewer.1 < 4
        || frame.is_some_and(|frame| {
            frame.size.0 > viewer.0.saturating_sub(2) || frame.size.1 > viewer.1.saturating_sub(2)
        });
    let border = if too_small {
        ansi::WARNING_BORDER
    } else {
        ansi::BORDER
    };

    let mut out = String::with_capacity(inner_w * inner_h * 4);
    if interactive {
        let keyboard = frame.map_or_else(KeyboardMode::empty, |f| f.keyboard_mode);
        let paste = frame.is_some_and(|f| f.bracketed_paste);
        let mouse = frame.map_or(MouseMode::None, |f| f.mouse_mode);
        if modes.applied.map(|(mode, _, _)| mode) != Some(keyboard) {
            out.push_str(&ansi::kitty_keyboard_mode(keyboard.bits()));
        }
        if modes.applied.map(|(_, paste, _)| paste) != Some(paste) {
            out.push_str(if paste {
                ansi::BRACKETED_PASTE_ENABLE
            } else {
                ansi::BRACKETED_PASTE_DISABLE
            });
        }
        if modes.applied.map(|(_, _, mouse)| mouse) != Some(mouse) {
            out.push_str(ansi::MOUSE_DISABLE);
            out.push_str(match mouse {
                MouseMode::None => "",
                MouseMode::Click => ansi::MOUSE_CLICK_ENABLE,
                MouseMode::Drag => ansi::MOUSE_DRAG_ENABLE,
                MouseMode::Motion => ansi::MOUSE_MOTION_ENABLE,
            });
        }
        modes.applied = Some((keyboard, paste, mouse));
    }
    out.push_str(ansi::HOME);
    header(&mut out, frame, session, inner_w, too_small, border);
    if let Some(f) = frame {
        content(&mut out, f, inner_w, inner_h, border);
    } else {
        placeholder(&mut out, inner_w, inner_h, border);
    }
    let detach_hint = if interactive {
        "┤ Ctrl+] detach ├"
    } else {
        "┤ q quit ├"
    };
    border_line(&mut out, '└', '┘', detach_hint, inner_w, false, border);
    out.push_str(ansi::ERASE_DISPLAY);
    out
}

fn header(
    out: &mut String,
    frame: Option<&Frame>,
    session: &str,
    inner_w: usize,
    too_small: bool,
    border: &str,
) {
    let title = match frame {
        Some(f) => {
            let shell = f.shell.map(|s| format!("{s} · ")).unwrap_or_default();
            let status = match f.exited {
                Some(code) => format!("exited {code}"),
                None => "live".to_string(),
            };
            let warning = if too_small { "! too small · " } else { "" };
            format!("┤ {warning}{shell}{}×{} · {status} ├", f.size.0, f.size.1)
        }
        None => format!("┤ {session} · no session ├"),
    };
    border_line(out, '┌', '┐', &title, inner_w, true, border);
}

fn content(out: &mut String, f: &Frame, inner_w: usize, inner_h: usize, border: &str) {
    let (cx, cy) = f.cursor;
    let show_cursor = f.exited.is_none();
    for y in 0..inner_h {
        out.push_str(border);
        out.push('│');
        out.push_str(ansi::RESET);
        let row = f.grid.get(y);
        let mut last: Option<Style> = None;
        for x in 0..inner_w {
            let mut cell = row.and_then(|r| r.get(x)).cloned().unwrap_or_default();
            if show_cursor && x as u16 == cx && y as u16 == cy {
                cell.attrs.toggle(Attrs::INVERSE);
            }
            let style = Style::from(&cell);
            if last.as_ref() != Some(&style) {
                out.push_str(&style.sgr());
                last = Some(style);
            }
            out.push_str(&cell.ch);
        }
        out.push_str(ansi::RESET);
        out.push_str(border);
        out.push('│');
        out.push_str(ansi::RESET);
        out.push_str(ansi::ERASE_LINE);
        out.push_str("\r\n");
    }
}

fn placeholder(out: &mut String, inner_w: usize, inner_h: usize, border: &str) {
    let msg = "no active session, run `tui-test open`";
    for y in 0..inner_h {
        out.push_str(border);
        out.push('│');
        out.push_str(ansi::RESET);
        if y == inner_h / 2 {
            let shown: String = msg.chars().take(inner_w).collect();
            let count = shown.chars().count();
            let pad = inner_w.saturating_sub(count) / 2;
            out.push_str(&" ".repeat(pad));
            out.push_str(&shown);
            out.push_str(&" ".repeat(inner_w.saturating_sub(pad + count)));
        } else {
            out.push_str(&" ".repeat(inner_w));
        }
        out.push_str(border);
        out.push('│');
        out.push_str(ansi::RESET);
        out.push_str(ansi::ERASE_LINE);
        out.push_str("\r\n");
    }
}

fn border_line(
    out: &mut String,
    left: char,
    right: char,
    title: &str,
    inner_w: usize,
    nl: bool,
    border: &str,
) {
    out.push_str(border);
    out.push(left);
    let tlen = title.chars().count();
    if tlen + 1 >= inner_w {
        out.extend(title.chars().take(inner_w));
    } else {
        out.push('─');
        out.push_str(title);
        for _ in 0..(inner_w - 1 - tlen) {
            out.push('─');
        }
    }
    out.push(right);
    out.push_str(ansi::RESET);
    out.push_str(ansi::ERASE_LINE);
    if nl {
        out.push_str("\r\n");
    }
}

#[derive(PartialEq, Clone)]
struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    underline: UnderlineStyle,
    underline_color: Option<Color>,
    attrs: Attrs,
}

impl Style {
    fn from(c: &EmuCell) -> Self {
        Style {
            fg: c.fg,
            bg: c.bg,
            underline: c.underline,
            underline_color: c.underline_color,
            attrs: c.attrs,
        }
    }

    fn sgr(&self) -> String {
        let mut s = String::from(ansi::SGR_START);
        for (attr, code) in [
            (Attrs::BOLD, "1"),
            (Attrs::DIM, "2"),
            (Attrs::ITALIC, "3"),
            (Attrs::BLINK, "5"),
            (Attrs::INVERSE, "7"),
            (Attrs::INVISIBLE, "8"),
            (Attrs::STRIKE, "9"),
        ] {
            if self.attrs.contains(attr) {
                s.push(';');
                s.push_str(code);
            }
        }
        let sub = match self.underline {
            UnderlineStyle::None => 0,
            UnderlineStyle::Single => 1,
            UnderlineStyle::Double => 2,
            UnderlineStyle::Curly => 3,
            UnderlineStyle::Dotted => 4,
            UnderlineStyle::Dashed => 5,
        };
        if sub != 0 {
            s.push_str(&format!(";4:{sub}"));
            // SGR 58 takes its arguments as colon-joined subparameters. Mixing
            // in a `;` would end the parameter early and the terminal would
            // read whatever follows as the underline's color instead.
            match self.underline_color {
                Some(Color::Rgb(r, g, b)) => s.push_str(&format!(";58:2::{r}:{g}:{b}")),
                Some(c) => s.push_str(&format!(";58:5:{}", c.to_index())),
                None => {}
            }
        }
        push_color(&mut s, self.fg, true);
        push_color(&mut s, self.bg, false);
        s.push('m');
        s
    }
}

fn push_color(s: &mut String, color: Option<Color>, fg: bool) {
    let base = if fg { 38 } else { 48 };
    match color {
        None => {}
        Some(Color::Rgb(r, g, b)) => s.push_str(&format!(";{base};2;{r};{g};{b}")),
        Some(c) => s.push_str(&format!(";{base};5;{}", c.to_index())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_frames_warn_until_the_child_fits_the_viewer() {
        use crate::terminal::{alacritty::AlacrittyEmu, cell::NamedColor, emu::Emulator};
        let viewer = (40, 12);
        let mut frame = Frame {
            grid: Vec::new(),
            cursor: (0, 0),
            size: (80, 24),
            keyboard_mode: KeyboardMode::empty(),
            bracketed_paste: false,
            mouse_mode: MouseMode::None,
            exited: None,
            shell: None,
        };
        for interactive in [false, true] {
            frame.size = (80, 24);
            let mut modes = ModeMirror::default();
            let clipped = render_frame(Some(&frame), viewer, "clipped", interactive, &mut modes);
            assert!(clipped.contains("! too small"));
            let mut emu =
                AlacrittyEmu::new(viewer.0, viewer.1, &crate::profile::Profile::default());
            emu.process(clipped.as_bytes());
            assert_eq!(
                emu.viewable_rows()[0][0].fg,
                Some(Color::Named(NamedColor::Yellow))
            );
            frame.size = content_size(viewer);
            let fitted = render_frame(Some(&frame), viewer, "clipped", interactive, &mut modes);
            assert!(!fitted.contains("! too small"));
            assert!(!fitted.contains(ansi::WARNING_BORDER));
        }
        assert!(render_frame(
            Some(&frame),
            (1, 1),
            "tiny",
            false,
            &mut ModeMirror::default()
        )
        .contains(ansi::WARNING_BORDER));
    }

    #[test]
    fn monitor_content_bounds_keep_the_childs_last_row_and_column_visible() {
        use crate::terminal::{alacritty::AlacrittyEmu, emu::Emulator};
        for viewer in [(80, 24), (40, 12)] {
            let size = content_size(viewer);
            let mut grid = vec![vec![EmuCell::blank(); size.0 as usize]; size.1 as usize];
            grid[size.1 as usize - 1][size.0 as usize - 1].ch = "Z".into();
            let frame = Frame {
                grid,
                cursor: (0, 0),
                size,
                keyboard_mode: KeyboardMode::empty(),
                bracketed_paste: false,
                mouse_mode: MouseMode::None,
                exited: None,
                shell: None,
            };
            let mut emu =
                AlacrittyEmu::new(viewer.0, viewer.1, &crate::profile::Profile::default());
            emu.process(
                render_frame(
                    Some(&frame),
                    viewer,
                    "edge",
                    true,
                    &mut ModeMirror::default(),
                )
                .as_bytes(),
            );
            assert_eq!(
                emu.viewable_rows()[viewer.1 as usize - 2][viewer.0 as usize - 2]
                    .ch
                    .as_str(),
                "Z"
            );
        }
    }

    /// `sgr` writes to the viewer's real terminal, so the only honest check is
    /// to feed it back through an emulator and see the same style come out.
    /// A malformed escape does not fail loudly; it silently reassigns the
    /// parameters that follow it, which is how `58` once swallowed the
    /// foreground color.
    #[test]
    fn sgr_survives_a_round_trip_through_the_emulator() {
        use crate::terminal::{alacritty::AlacrittyEmu, cell::NamedColor, emu::Emulator};

        let styles = [
            Style {
                fg: Some(Color::Named(NamedColor::Red)),
                bg: Some(Color::Idx(196)),
                underline: UnderlineStyle::Curly,
                underline_color: Some(Color::Rgb(1, 2, 3)),
                attrs: Attrs::BOLD | Attrs::ITALIC | Attrs::STRIKE,
            },
            Style {
                fg: Some(Color::Rgb(9, 8, 7)),
                bg: None,
                underline: UnderlineStyle::Dotted,
                underline_color: Some(Color::Idx(33)),
                attrs: Attrs::DIM,
            },
            Style {
                fg: None,
                bg: Some(Color::Named(NamedColor::BrightWhite)),
                underline: UnderlineStyle::Single,
                underline_color: None,
                attrs: Attrs::empty(),
            },
        ];

        for want in styles {
            let mut emu = AlacrittyEmu::new(10, 2, &crate::profile::Profile::default());
            emu.process(want.sgr().as_bytes());
            emu.process(b"x");
            let got = Style::from(&emu.viewable_rows()[0][0]);
            assert!(
                got == want,
                "{:?} round-tripped to fg={:?} bg={:?} underline={:?}/{:?} attrs={:?}",
                want.sgr(),
                got.fg,
                got.bg,
                got.underline,
                got.underline_color,
                got.attrs,
            );
        }
    }
}
