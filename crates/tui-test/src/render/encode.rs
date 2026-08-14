use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
#[cfg(test)]
use std::time::Duration;

use crate::api::RecordingFormat;
use crate::record::frames::Frame;

use super::raster::FrameRenderer;

pub(crate) fn encode(
    path: &Path,
    format: RecordingFormat,
    frames: &[Frame],
    renderer: &mut dyn FrameRenderer,
) -> anyhow::Result<()> {
    if frames.is_empty() {
        anyhow::bail!("recording timeline contains no frames");
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    match format {
        RecordingFormat::Gif => encode_gif(path, frames, renderer),
        RecordingFormat::Cast => anyhow::bail!("cast recordings do not require animation encoding"),
    }
}

fn encode_gif(
    path: &Path,
    frames: &[Frame],
    renderer: &mut dyn FrameRenderer,
) -> anyhow::Result<()> {
    let (width, height) = renderer.pixel_size();
    let width: u16 = width.try_into()?;
    let height: u16 = height.try_into()?;
    let output = BufWriter::new(File::create(path)?);
    let mut encoder = gif::Encoder::new(output, width, height, &[])?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    for step in gif_timeline(frames) {
        let mut pixels = renderer.render(&frames[step.frame])?.into_raw();
        let mut frame = gif::Frame::from_rgba_speed(width, height, &mut pixels, 10);
        frame.delay = step.delay;
        encoder.write_frame(&frame)?;
    }
    let mut output = encoder.into_inner()?;
    output.flush()?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GifStep {
    frame: usize,
    delay: u16,
}

fn gif_timeline(frames: &[Frame]) -> Vec<GifStep> {
    const TICK_NANOS: u128 = 10_000_000;

    let mut output: Vec<GifStep> = Vec::new();
    let mut elapsed = 0u128;
    let mut emitted = 0u128;
    for (index, frame) in frames.iter().enumerate() {
        elapsed = elapsed.saturating_add(frame.duration.as_nanos());
        let rounded = elapsed.saturating_add(TICK_NANOS / 2) / TICK_NANOS;
        if rounded <= emitted {
            continue;
        }
        let mut delay = rounded - emitted;
        while delay > 0 {
            let chunk = delay.min(u128::from(u16::MAX)) as u16;
            output.push(GifStep {
                frame: index,
                delay: chunk,
            });
            delay -= u128::from(chunk);
        }
        emitted = rounded;
    }
    if output.is_empty() {
        output.push(GifStep {
            frame: frames.len().saturating_sub(1),
            delay: 1,
        });
    } else if output
        .last()
        .is_some_and(|step| step.frame + 1 < frames.len())
    {
        let last_frame = frames.len() - 1;
        if let Some(previous) = output.iter_mut().rev().find(|step| step.delay > 1) {
            previous.delay -= 1;
            output.push(GifStep {
                frame: last_frame,
                delay: 1,
            });
        } else if let Some(previous) = output.last_mut() {
            previous.frame = last_frame;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::profile::Profile;
    use crate::render::raster::{FrameRenderer, GridRenderer};
    use crate::render::svg::RenderState;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::{Color, EmuCell};

    #[test]
    fn gif_timing_diffuses_thirty_fps_rounding_error() {
        let frames = (0..30)
            .map(|_| frame(Color::Rgb(1, 2, 3), Duration::from_secs_f64(1.0 / 30.0)))
            .collect::<Vec<_>>();
        let timeline = gif_timeline(&frames);
        assert!(timeline.iter().all(|step| step.delay > 0));
        assert_eq!(
            timeline
                .iter()
                .map(|step| u32::from(step.delay))
                .sum::<u32>(),
            100
        );
        assert!(timeline.iter().any(|step| step.delay == 4));
    }

    #[test]
    fn sub_centisecond_frames_are_coalesced_without_zero_delays() {
        let frames = (0..4)
            .map(|index| frame(Color::Rgb(index, 0, 0), Duration::from_millis(4)))
            .collect::<Vec<_>>();
        let timeline = gif_timeline(&frames);
        assert!(timeline.iter().all(|step| step.delay > 0));
        assert_eq!(timeline.last().unwrap().frame, frames.len() - 1);
    }

    #[test]
    fn a_short_frame_does_not_replace_an_already_timed_frame() {
        let frames = [
            (0, Duration::from_millis(100)),
            (1, Duration::from_millis(4)),
            (2, Duration::from_millis(100)),
        ]
        .into_iter()
        .map(|(red, duration)| frame(Color::Rgb(red, 0, 0), duration))
        .collect::<Vec<_>>();
        assert_eq!(
            gif_timeline(&frames),
            vec![
                GifStep {
                    frame: 0,
                    delay: 10
                },
                GifStep {
                    frame: 2,
                    delay: 10
                }
            ]
        );
    }

    #[test]
    fn a_short_final_frame_gets_a_visible_tick_without_changing_total_time() {
        let frames = [
            (0, Duration::from_millis(100)),
            (1, Duration::from_millis(4)),
        ]
        .into_iter()
        .map(|(red, duration)| frame(Color::Rgb(red, 0, 0), duration))
        .collect::<Vec<_>>();
        assert_eq!(
            gif_timeline(&frames),
            vec![
                GifStep { frame: 0, delay: 9 },
                GifStep { frame: 1, delay: 1 }
            ]
        );
    }

    #[test]
    fn gif_round_trips_dimensions_delays_and_color() {
        let path = temp_path("gif");
        let frames = sample_frames();
        let scale = 2;
        let mut renderer = GridRenderer::with_scale(1, 1, scale);
        encode(&path, RecordingFormat::Gif, &frames, &mut renderer).unwrap();

        let decoded = decode_gif(&path, 20 * scale, 48 * scale);
        assert_eq!(decoded.frames, 2);
        assert_eq!(decoded.dimensions, renderer.pixel_size());
        assert_eq!(decoded.delay, Duration::from_millis(400));
        for (actual, expected) in decoded.pixel[..3].iter().zip([200u8, 10, 20]) {
            assert!(actual.abs_diff(expected) <= 3);
        }
        std::fs::remove_file(path).unwrap();
    }

    fn sample_frames() -> Vec<Frame> {
        [
            (Color::Rgb(200, 10, 20), Duration::from_millis(100)),
            (Color::Rgb(10, 20, 200), Duration::from_millis(300)),
        ]
        .into_iter()
        .map(|(background, duration)| frame(background, duration))
        .collect()
    }

    fn frame(background: Color, duration: Duration) -> Frame {
        let emulator = AlacrittyEmu::new(1, 1, &Profile::default());
        Frame {
            grid: sample_grid(background),
            title: None,
            duration,
            render_state: RenderState::capture(&emulator),
            cursor: None,
        }
    }

    fn sample_grid(background: Color) -> Vec<Vec<EmuCell>> {
        vec![vec![EmuCell {
            bg: Some(background),
            ..EmuCell::blank()
        }]]
    }

    struct DecodedGif {
        frames: usize,
        dimensions: (u32, u32),
        delay: Duration,
        pixel: [u8; 4],
    }

    fn decode_gif(path: &Path, x: u32, y: u32) -> DecodedGif {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(File::open(path).unwrap()).unwrap();
        let dimensions = (u32::from(decoder.width()), u32::from(decoder.height()));
        let mut frames = 0;
        let mut delay = Duration::ZERO;
        let mut pixel = [0; 4];
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            if frames == 0 {
                let offset = ((y * dimensions.0 + x) * 4) as usize;
                pixel.copy_from_slice(&frame.buffer[offset..offset + 4]);
            }
            frames += 1;
            delay += Duration::from_millis(u64::from(frame.delay) * 10);
        }
        DecodedGif {
            frames,
            dimensions,
            delay,
            pixel,
        }
    }

    fn temp_path(extension: &str) -> std::path::PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "tui-test-animation-{}-{}.{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            extension
        ))
    }
}
