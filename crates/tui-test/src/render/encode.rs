use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::api::RecordingFormat;
use crate::record::frames::Frame;

use super::raster::FrameRenderer;

pub(crate) fn encode(
    path: &Path,
    format: RecordingFormat,
    frames: &[Frame],
    renderer: &mut dyn FrameRenderer,
    fps: u8,
    ffmpeg_path: Option<&Path>,
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
        RecordingFormat::Apng => encode_apng(path, frames, renderer),
        RecordingFormat::Gif => encode_gif(path, frames, renderer),
        RecordingFormat::Mp4 => {
            let ffmpeg_path = ffmpeg_path
                .ok_or_else(|| anyhow::anyhow!("MP4 recording is missing its ffmpeg executable"))?;
            encode_mp4(path, frames, renderer, fps, ffmpeg_path)
        }
        RecordingFormat::Cast => anyhow::bail!("cast recordings do not require animation encoding"),
    }
}

fn encode_mp4(
    path: &Path,
    frames: &[Frame],
    renderer: &mut dyn FrameRenderer,
    fps: u8,
    ffmpeg_path: &Path,
) -> anyhow::Result<()> {
    let timeline = mp4_timeline(frames, fps)?;
    let (width, height) = renderer.pixel_size();
    let mut child = mp4_command(ffmpeg_path, path, width, height, fps)
        .spawn()
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to start ffmpeg at {}: {error}",
                ffmpeg_path.display()
            )
        })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to open ffmpeg input"))?;
    let write_result = stream_mp4_frames(&mut stdin, &timeline, frames, renderer);
    drop(stdin);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            anyhow::bail!("ffmpeg failed with {}", output.status);
        }
        anyhow::bail!("ffmpeg failed with {}: {stderr}", output.status);
    }
    write_result
}

fn mp4_command(
    ffmpeg_path: &Path,
    output_path: &Path,
    width: u32,
    height: u32,
    fps: u8,
) -> Command {
    let mut command = Command::new(ffmpeg_path);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pixel_format")
        .arg("rgba")
        .arg("-video_size")
        .arg(format!("{width}x{height}"))
        .arg("-framerate")
        .arg(fps.to_string())
        .arg("-i")
        .arg("pipe:0")
        .arg("-an")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-crf")
        .arg("18")
        .arg("-vf")
        .arg("pad=ceil(iw/2)*2:ceil(ih/2)*2")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-f")
        .arg("mp4")
        .arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mp4Step {
    frame: usize,
    repeats: usize,
}

fn mp4_timeline(frames: &[Frame], fps: u8) -> anyhow::Result<Vec<Mp4Step>> {
    if fps == 0 {
        anyhow::bail!("recording fps must be greater than zero");
    }
    let mut output: Vec<Mp4Step> = Vec::new();
    let mut elapsed = 0u128;
    let mut emitted = 0u128;
    for (index, frame) in frames.iter().enumerate() {
        elapsed = elapsed
            .checked_add(frame.duration.as_nanos())
            .ok_or_else(|| anyhow::anyhow!("MP4 recording timeline is too long"))?;
        let scaled = elapsed
            .checked_mul(u128::from(fps))
            .ok_or_else(|| anyhow::anyhow!("MP4 recording timeline is too long"))?;
        let rounded = scaled.saturating_add(500_000_000) / 1_000_000_000;
        if rounded <= emitted {
            continue;
        }
        output.push(Mp4Step {
            frame: index,
            repeats: (rounded - emitted)
                .try_into()
                .map_err(|_| anyhow::anyhow!("MP4 recording timeline is too long"))?,
        });
        emitted = rounded;
    }
    if output.is_empty() {
        output.push(Mp4Step {
            frame: frames.len().saturating_sub(1),
            repeats: 1,
        });
    } else if output
        .last()
        .is_some_and(|step| step.frame + 1 < frames.len())
    {
        let last_frame = frames.len() - 1;
        if let Some(previous) = output.iter_mut().rev().find(|step| step.repeats > 1) {
            previous.repeats -= 1;
            output.push(Mp4Step {
                frame: last_frame,
                repeats: 1,
            });
        } else if let Some(previous) = output.last_mut() {
            previous.frame = last_frame;
        }
    }
    Ok(output)
}

fn stream_mp4_frames(
    output: &mut dyn Write,
    timeline: &[Mp4Step],
    frames: &[Frame],
    renderer: &mut dyn FrameRenderer,
) -> anyhow::Result<()> {
    for step in timeline {
        let pixels = renderer.render(&frames[step.frame])?;
        for _ in 0..step.repeats {
            output.write_all(pixels.as_raw())?;
        }
    }
    output.flush()?;
    Ok(())
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

fn encode_apng(
    path: &Path,
    frames: &[Frame],
    renderer: &mut dyn FrameRenderer,
) -> anyhow::Result<()> {
    let (width, height) = renderer.pixel_size();
    let output = BufWriter::new(File::create(path)?);
    let mut encoder = png::Encoder::new(output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(frames.len().try_into()?, 0)?;
    encoder.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
    let mut writer = encoder.write_header()?;
    for frame in frames {
        let image = renderer.render(frame)?;
        let (delay_num, delay_den) = apng_delay(frame.duration);
        writer.set_frame_delay(delay_num, delay_den)?;
        writer.write_image_data(image.as_raw())?;
    }
    writer.finish()?;
    Ok(())
}

fn apng_delay(duration: Duration) -> (u16, u16) {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        return (1, u16::MAX);
    }
    let denominator = (f64::from(u16::MAX) / seconds)
        .floor()
        .clamp(1.0, f64::from(u16::MAX)) as u16;
    let numerator = (seconds * f64::from(denominator))
        .round()
        .clamp(1.0, f64::from(u16::MAX)) as u16;
    (numerator, denominator)
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
    use std::io::BufReader;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::profile::Profile;
    use crate::render::raster::{FrameRenderer, GridRenderer};
    use crate::render::svg::RenderState;
    use crate::terminal::alacritty::AlacrittyEmu;
    use crate::terminal::cell::{Color, EmuCell};

    #[test]
    fn apng_delay_preserves_common_recording_intervals() {
        for duration in [
            Duration::from_secs_f64(1.0 / 30.0),
            Duration::from_millis(2500),
            Duration::from_secs(5),
        ] {
            let (numerator, denominator) = apng_delay(duration);
            let actual = f64::from(numerator) / f64::from(denominator);
            assert!((actual - duration.as_secs_f64()).abs() < 0.000_1);
        }
    }

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
    fn mp4_timing_diffuses_thirty_fps_rounding_error() {
        let frames = (0..30)
            .map(|_| frame(Color::Rgb(1, 2, 3), Duration::from_secs_f64(1.0 / 30.0)))
            .collect::<Vec<_>>();
        let timeline = mp4_timeline(&frames, 30).unwrap();
        assert!(timeline.iter().all(|step| step.repeats > 0));
        assert_eq!(timeline.iter().map(|step| step.repeats).sum::<usize>(), 30);
    }

    #[test]
    fn a_short_final_mp4_frame_remains_visible() {
        let frames = [
            (0, Duration::from_millis(100)),
            (1, Duration::from_millis(4)),
        ]
        .into_iter()
        .map(|(red, duration)| frame(Color::Rgb(red, 0, 0), duration))
        .collect::<Vec<_>>();
        assert_eq!(
            mp4_timeline(&frames, 30).unwrap(),
            vec![
                Mp4Step {
                    frame: 0,
                    repeats: 2
                },
                Mp4Step {
                    frame: 1,
                    repeats: 1
                }
            ]
        );
    }

    #[test]
    fn apng_and_gif_round_trip_dimensions_delays_and_color() {
        for format in [RecordingFormat::Apng, RecordingFormat::Gif] {
            let path = temp_path(match format {
                RecordingFormat::Apng => "png",
                RecordingFormat::Gif => "gif",
                RecordingFormat::Mp4 => unreachable!(),
                RecordingFormat::Cast => unreachable!(),
            });
            let frames = sample_frames();
            let scale = 2;
            let mut renderer = GridRenderer::with_scale(1, 1, scale);
            encode(&path, format, &frames, &mut renderer, 30, None).unwrap();

            match format {
                RecordingFormat::Apng => {
                    let bytes = std::fs::read(&path).unwrap();
                    let chunks = png_chunks(&bytes);
                    assert_eq!(png_dimensions(&chunks), renderer.pixel_size());
                    assert_eq!(png_animation_frames(&chunks), 2);
                    assert!(
                        png_total_delay(&chunks).abs_diff(Duration::from_millis(400))
                            <= Duration::from_micros(100)
                    );
                    let pixel = decode_first_png_pixel(
                        &path,
                        (crate::render::raster::CANVAS_PADDING + 20) * scale,
                        (crate::render::raster::CANVAS_PADDING + 48) * scale,
                    );
                    assert_eq!(&pixel[..3], &[200, 10, 20]);
                }
                RecordingFormat::Gif => {
                    let decoded = decode_gif(
                        &path,
                        (crate::render::raster::CANVAS_PADDING + 20) * scale,
                        (crate::render::raster::CANVAS_PADDING + 48) * scale,
                    );
                    assert_eq!(decoded.frames, 2);
                    assert_eq!(decoded.dimensions, renderer.pixel_size());
                    assert_eq!(decoded.delay, Duration::from_millis(400));
                    for (actual, expected) in decoded.pixel[..3].iter().zip([200u8, 10, 20]) {
                        assert!(actual.abs_diff(expected) <= 3);
                    }
                }
                RecordingFormat::Mp4 => unreachable!(),
                RecordingFormat::Cast => unreachable!(),
            }
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn repeated_apng_encodes_are_byte_identical() {
        let first = temp_path("png");
        let second = temp_path("png");
        let frames = sample_frames();
        for path in [&first, &second] {
            let mut renderer = GridRenderer::with_scale(1, 1, 2);
            encode(
                path,
                RecordingFormat::Apng,
                &frames,
                &mut renderer,
                30,
                None,
            )
            .unwrap();
        }
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
    }

    #[test]
    fn mp4_requires_an_ffmpeg_path() {
        let path = temp_path("mp4");
        let frames = sample_frames();
        let mut renderer = GridRenderer::with_scale(1, 1, 2);
        let error = encode(
            &path,
            RecordingFormat::Mp4,
            &frames,
            &mut renderer,
            30,
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("ffmpeg"));
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

    fn decode_first_png_pixel(path: &Path, x: u32, y: u32) -> [u8; 4] {
        let decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).unwrap();
        let offset = ((y * info.width + x) * 4) as usize;
        buffer[offset..offset + 4].try_into().unwrap()
    }

    fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let mut chunks = Vec::new();
        let mut offset = 8;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = bytes[offset + 4..offset + 8].try_into().unwrap();
            let data = &bytes[offset + 8..offset + 8 + length];
            chunks.push((kind, data));
            offset += length + 12;
        }
        chunks
    }

    fn png_dimensions(chunks: &[([u8; 4], &[u8])]) -> (u32, u32) {
        let data = chunks.iter().find(|(kind, _)| kind == b"IHDR").unwrap().1;
        (
            u32::from_be_bytes(data[..4].try_into().unwrap()),
            u32::from_be_bytes(data[4..8].try_into().unwrap()),
        )
    }

    fn png_animation_frames(chunks: &[([u8; 4], &[u8])]) -> u32 {
        let data = chunks.iter().find(|(kind, _)| kind == b"acTL").unwrap().1;
        u32::from_be_bytes(data[..4].try_into().unwrap())
    }

    fn png_total_delay(chunks: &[([u8; 4], &[u8])]) -> Duration {
        chunks
            .iter()
            .filter(|(kind, _)| kind == b"fcTL")
            .map(|(_, data)| {
                let numerator = u16::from_be_bytes(data[20..22].try_into().unwrap());
                let denominator = u16::from_be_bytes(data[22..24].try_into().unwrap()).max(1);
                Duration::from_secs_f64(f64::from(numerator) / f64::from(denominator))
            })
            .sum()
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
