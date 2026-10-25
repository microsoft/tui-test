//! U06 pixel comparison tests (U06-A through U06-E).
//!
//! The PNG helpers below encode and decode a small subset of PNG (8-bit
//! RGBA, filter zero) with only the standard library. They exist so U06-B
//! decodes two genuinely different PNG byte streams without a new test
//! dependency.

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
    let plain = encode_png(width, height, &pixels, &[], 1);
    let tagged = encode_png(
        width,
        height,
        &pixels,
        &[("Comment", "second encoder"), ("Title", "u06-b")],
        3,
    );
    assert_ne!(plain, tagged);

    let decoded_plain = decode_png(&plain).unwrap();
    let decoded_tagged = decode_png(&tagged).unwrap();
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
    for (x, y) in changed_positions {
        assert!((bounds.min_x..=bounds.max_x).contains(&x));
        assert!((bounds.min_y..=bounds.max_y).contains(&y));
        assert_eq!(
            pixel_at(&result.difference_image.clone().unwrap(), width, x, y),
            MAGENTA
        );
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

// --- Minimal PNG codec (8-bit RGBA, filter zero) for U06-B. ---

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut check = Vec::with_capacity(4 + data.len());
    check.extend_from_slice(kind);
    check.extend_from_slice(data);
    out.extend_from_slice(&crc32(&check).to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    let mut low: u32 = 1;
    let mut high: u32 = 0;
    for byte in data {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

fn encode_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    text: &[(&str, &str)],
    idat_parts: usize,
) -> Vec<u8> {
    let mut raw = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib wrapper around one stored deflate block.
    let mut compressed = vec![0x78, 0x01, 0x01];
    let len = raw.len() as u16;
    compressed.extend_from_slice(&len.to_le_bytes());
    compressed.extend_from_slice(&(!len).to_le_bytes());
    compressed.extend_from_slice(&raw);
    compressed.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr, &mut out);
    for (keyword, value) in text {
        let mut data = Vec::new();
        data.extend_from_slice(keyword.as_bytes());
        data.push(0);
        data.extend_from_slice(value.as_bytes());
        chunk(b"tEXt", &data, &mut out);
    }
    let part_len = compressed.len().div_ceil(idat_parts.max(1));
    for part in compressed.chunks(part_len.max(1)) {
        chunk(b"IDAT", part, &mut out);
    }
    chunk(b"IEND", &[], &mut out);
    out
}

fn read_u32(data: &[u8], position: &mut usize) -> Option<u32> {
    let end = position.checked_add(4)?;
    let bytes = data.get(*position..end)?;
    *position = end;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn decode_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.get(..8) != Some(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return None;
    }
    let mut position = 8;
    let (mut width, mut height) = (0u32, 0u32);
    let mut found_ihdr = false;
    let mut idat = Vec::new();
    loop {
        let length = read_u32(data, &mut position)? as usize;
        let end = position.checked_add(8)?.checked_add(length)?;
        let kind = data.get(position..position + 4)?;
        let body = data.get(position + 4..position + 4 + length)?;
        position = end;
        match kind {
            [73, 72, 68, 82] => {
                if found_ihdr || length != 13 {
                    return None;
                }
                found_ihdr = true;
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                if body[8] != 8 || body[9] != 6 {
                    return None;
                }
            }
            [73, 68, 65, 84] => idat.extend_from_slice(body),
            [73, 69, 78, 68] => break,
            _ => {}
        }
    }
    if !found_ihdr || width == 0 || height == 0 {
        return None;
    }
    // zlib header for no compression, then one stored block.
    if idat.get(..2) != Some(&[0x78, 0x01]) || idat.get(2) != Some(&0x01) {
        return None;
    }
    let len = u16::from_le_bytes([idat[3], idat[4]]) as usize;
    let nlen = u16::from_le_bytes([idat[5], idat[6]]) as usize;
    if len ^ nlen != 0xFFFF {
        return None;
    }
    let body = idat.get(7..7 + len)?;
    let stride = width as usize * 4;
    let mut pixels = vec![0u8; stride * height as usize];
    let mut offset = 0;
    for row in pixels.chunks_exact_mut(stride) {
        let scanline = body.get(offset..offset + 1 + stride)?;
        if scanline[0] != 0 {
            return None;
        }
        row.copy_from_slice(&scanline[1..]);
        offset += 1 + stride;
    }
    Some((width, height, pixels))
}
