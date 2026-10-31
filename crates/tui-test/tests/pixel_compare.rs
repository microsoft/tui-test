//! U06 pixel comparison tests (U06-A through U06-E).
//!
//! The PNG helpers below use the workspace `png` crate (also a
//! dev-dependency, as in `tui-test-cli`) so U06-B decodes two genuinely
//! different PNG byte streams with the real decoder.

use tui_test::pixel::{compare_rgba, PixelError, PixelInput};

const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn pixel_at(image: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let start = ((u64::from(y) * u64::from(width) + u64::from(x)) * 4) as usize;
    [
        image[start],
        image[start + 1],
        image[start + 2],
        image[start + 3],
    ]
}

fn rgb(r: u8, g: u8, b: u8) -> [u8; 4] {
    [r, g, b, 255]
}

fn base_image() -> (u32, u32, Vec<u8>) {
    let cells = [
        rgb(10, 20, 30),
        rgb(40, 50, 60),
        rgb(70, 80, 90),
        rgb(100, 110, 120),
        rgb(130, 140, 150),
        rgb(160, 170, 180),
    ];
    let mut bytes = Vec::with_capacity(24);
    for cell in cells {
        bytes.extend_from_slice(&cell);
    }
    (3, 2, bytes)
}

// U06-A: C08 pixel equality, one changed channel, one changed alpha value.
#[test]
fn u06_a_equal_passes_single_channel_and_alpha_fail() {
    let (width, height, first) = base_image();

    let same = compare_rgba(width, height, &first, width, height, &first).unwrap();
    assert!(same.is_equal());
    assert_eq!(same.changed_pixels, 0);
    assert_eq!(same.compared_pixels, 6);
    assert_eq!(same.bounds, None);
    let image = same.difference_image.unwrap();
    assert_eq!(image.len(), first.len());
    let (cells, _) = image.as_chunks::<4>();
    assert!(cells.iter().all(|pixel| *pixel == CLEAR));

    // One changed green channel at (1, 0) and one changed alpha at (2, 1).
    let mut second = first.clone();
    // Green channel of pixel (1, 0); alpha channel of pixel (2, 1).
    second[5] = 21;
    second[23] = 0;
    let changed = compare_rgba(width, height, &first, width, height, &second).unwrap();
    assert!(!changed.is_equal());
    assert_eq!(changed.changed_pixels, 2);
    assert_eq!(changed.compared_pixels, 6);
    let bounds = changed.bounds.unwrap();
    assert_eq!((bounds.min_x, bounds.min_y), (1, 0));
    assert_eq!((bounds.max_x, bounds.max_y), (2, 1));
    let image = changed.difference_image.unwrap();
    assert_eq!(pixel_at(&image, width, 1, 0), MAGENTA);
    assert_eq!(pixel_at(&image, width, 2, 1), MAGENTA);
    assert_eq!(pixel_at(&image, width, 0, 0), CLEAR);
    assert_eq!(pixel_at(&image, width, 0, 1), CLEAR);

    // RGB differences count even when alpha is zero in both images.
    let mut third = first.clone();
    third[0] = 11;
    third[3] = 0;
    let mut fourth = first.clone();
    fourth[3] = 0;
    let transparent = compare_rgba(width, height, &third, width, height, &fourth).unwrap();
    assert!(!transparent.is_equal());
    assert_eq!(transparent.changed_pixels, 1);
}

// U06-B: equal decoded pixels pass with different PNG metadata and chunking.
#[test]
fn u06_b_same_pixels_pass_with_different_png_encoding() {
    let (width, height, pixels) = base_image();
    let plain = encode_png(
        width,
        height,
        &pixels,
        &[],
        png::Compression::Fast,
        png::AdaptiveFilterType::NonAdaptive,
    );
    let tagged = encode_png(
        width,
        height,
        &pixels,
        &[("Comment", "second encoder"), ("Title", "u06-b")],
        png::Compression::Best,
        png::AdaptiveFilterType::Adaptive,
    );
    assert_ne!(plain, tagged);

    let decoded_plain = decode_png(&plain);
    let decoded_tagged = decode_png(&tagged);
    assert_eq!((decoded_plain.0, decoded_plain.1), (width, height));
    assert_eq!(decoded_plain.2, pixels);
    assert_eq!(decoded_tagged.2, pixels);

    let result = compare_rgba(
        decoded_plain.0,
        decoded_plain.1,
        &decoded_plain.2,
        decoded_tagged.0,
        decoded_tagged.1,
        &decoded_tagged.2,
    )
    .unwrap();
    assert!(result.is_equal());
    assert_eq!(result.changed_pixels, 0);
    assert_eq!(result.bounds, None);
}

// U06-C: empty dimensions, invalid lengths, and arithmetic overflow fail.
#[test]
fn u06_c_invalid_buffers_fail_typed() {
    let (width, height, pixels) = base_image();

    assert_eq!(
        compare_rgba(0, height, &[], width, height, &pixels).unwrap_err(),
        PixelError::EmptyDimensions {
            image: PixelInput::First,
            width: 0,
            height,
        }
    );
    assert_eq!(
        compare_rgba(width, height, &pixels, width, 0, &[]).unwrap_err(),
        PixelError::EmptyDimensions {
            image: PixelInput::Second,
            width,
            height: 0,
        }
    );

    let mut short = pixels.clone();
    short.pop();
    assert_eq!(
        compare_rgba(width, height, &short, width, height, &pixels).unwrap_err(),
        PixelError::LengthMismatch {
            image: PixelInput::First,
            width,
            height,
            expected: 24,
            actual: 23,
        }
    );

    let mut long = pixels.clone();
    long.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(
        compare_rgba(width, height, &pixels, width, height, &long).unwrap_err(),
        PixelError::LengthMismatch {
            image: PixelInput::Second,
            width,
            height,
            expected: 24,
            actual: 28,
        }
    );

    assert_eq!(
        compare_rgba(u32::MAX, u32::MAX, &[], 1, 1, &[0, 0, 0, 0]).unwrap_err(),
        PixelError::PixelCountOverflow {
            image: PixelInput::First,
            width: u32::MAX,
            height: u32::MAX,
        }
    );
}

// U06-D: the difference bounds contain each changed pixel.
#[test]
fn u06_d_bounds_enclose_every_changed_pixel() {
    let width = 5u32;
    let height = 4u32;
    let first = vec![9u8; 5 * 4 * 4];
    let mut second = first.clone();
    let changed_positions = [(0u32, 0u32), (4, 3), (2, 1)];
    for (x, y) in changed_positions {
        let start = ((u64::from(y) * u64::from(width) + u64::from(x)) * 4) as usize;
        second[start] = 10;
    }
    let result = compare_rgba(width, height, &first, width, height, &second).unwrap();
    assert_eq!(result.changed_pixels, 3);
    assert_eq!(result.compared_pixels, 20);
    let bounds = result.bounds.unwrap();
    assert_eq!((bounds.min_x, bounds.min_y), (0, 0));
    assert_eq!((bounds.max_x, bounds.max_y), (4, 3));
    let difference_image = result.difference_image.clone().unwrap();
    for (x, y) in changed_positions {
        assert!((bounds.min_x..=bounds.max_x).contains(&x));
        assert!((bounds.min_y..=bounds.max_y).contains(&y));
        assert_eq!(pixel_at(&difference_image, width, x, y), MAGENTA);
    }

    // One changed pixel gives a single-point rectangle.
    let mut single = first.clone();
    single[0] = 10;
    let result = compare_rgba(width, height, &first, width, height, &single).unwrap();
    let bounds = result.bounds.unwrap();
    assert_eq!((bounds.min_x, bounds.min_y), (0, 0));
    assert_eq!((bounds.max_x, bounds.max_y), (0, 0));
}

// U06-E: a dimension mismatch is a different result, never a zero success.
#[test]
fn u06_e_dimension_mismatch_is_different() {
    let first = vec![7u8; 4 * 4];
    let second = vec![7u8; 2 * 2 * 4];
    let result = compare_rgba(4, 1, &first, 2, 2, &second).unwrap();
    assert!(!result.is_equal());
    assert_ne!(result.changed_pixels, 0);
    assert_eq!(result.compared_pixels, 0);
    assert_eq!(result.bounds, None);
    assert_eq!(result.difference_image, None);

    let bigger = vec![7u8; 3 * 3 * 4];
    let result = compare_rgba(2, 2, &second, 3, 3, &bigger).unwrap();
    assert!(!result.is_equal());
    assert_eq!(result.changed_pixels, 9);
    assert_eq!(result.difference_image, None);
}

// --- PNG helpers (8-bit RGBA) for U06-B, via the workspace `png` crate. ---

fn encode_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    text: &[(&str, &str)],
    compression: png::Compression,
    filter: png::AdaptiveFilterType,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(compression);
    encoder.set_adaptive_filter(filter);
    for (keyword, value) in text {
        encoder
            .add_text_chunk((*keyword).to_owned(), (*value).to_owned())
            .unwrap();
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(pixels).unwrap();
    writer.finish().unwrap();
    out
}

fn decode_png(data: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(data);
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    (info.width, info.height, pixels)
}
