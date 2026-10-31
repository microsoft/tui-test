//! Pure pixel comparison over decoded RGBA8 data.
//!
//! [`compare_rgba`] accepts image dimensions with decoded RGBA8 bytes in
//! straight-alpha sRGB order. It compares the four bytes of each pixel for
//! equality, including the RGB bytes of fully transparent pixels. It applies
//! no tolerance and no masks.
//!
//! The function reads no baseline file, writes no baseline file, and approves
//! no snapshot. Callers supply decoded bytes only, so PNG metadata and PNG
//! compression settings cannot change the result.

use std::fmt;

/// Selects one of the two input images in [`PixelError`] values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelInput {
    /// The first image (`width_a`, `height_a`, `bytes_a`).
    First,
    /// The second image (`width_b`, `height_b`, `bytes_b`).
    Second,
}

impl fmt::Display for PixelInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixelInput::First => formatter.write_str("first image"),
            PixelInput::Second => formatter.write_str("second image"),
        }
    }
}

/// Typed failure for invalid pixel buffers.
///
/// A dimension mismatch between two valid buffers is not an error. It returns
/// a [`PixelComparison`] that reads as different. See [`compare_rgba`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PixelError {
    /// One dimension is zero, so the buffer holds no pixel.
    EmptyDimensions {
        /// The image with the empty dimensions.
        image: PixelInput,
        /// The rejected width.
        width: u32,
        /// The rejected height.
        height: u32,
    },
    /// The byte slice length differs from `width * height * 4`.
    LengthMismatch {
        /// The image with the wrong length.
        image: PixelInput,
        /// The validated width.
        width: u32,
        /// The validated height.
        height: u32,
        /// The required length in bytes.
        expected: usize,
        /// The supplied length in bytes.
        actual: usize,
    },
    /// `width * height * 4` does not fit in `usize`.
    PixelCountOverflow {
        /// The image with the overflowing dimensions.
        image: PixelInput,
        /// The rejected width.
        width: u32,
        /// The rejected height.
        height: u32,
    },
}

impl fmt::Display for PixelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixelError::EmptyDimensions {
                image,
                width,
                height,
            } => {
                write!(
                    formatter,
                    "{image} has empty dimensions ({width}x{height}); \
                     both dimensions must be nonzero"
                )
            }
            PixelError::LengthMismatch {
                image,
                width,
                height,
                expected,
                actual,
            } => {
                write!(
                    formatter,
                    "{image} has length {actual} but {width}x{height} RGBA8 \
                     requires {expected} bytes"
                )
            }
            PixelError::PixelCountOverflow {
                image,
                width,
                height,
            } => {
                write!(
                    formatter,
                    "{image} dimensions {width}x{height} overflow the buffer length"
                )
            }
        }
    }
}

impl std::error::Error for PixelError {}

/// Smallest rectangle that contains every changed pixel.
///
/// Coordinates are zero-based pixel positions with inclusive edges: `min_x`
/// and `min_y` hold the top-left changed pixel, `max_x` and `max_y` hold the
/// bottom-right changed pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PixelBounds {
    /// The smallest changed x coordinate.
    pub min_x: u32,
    /// The smallest changed y coordinate.
    pub min_y: u32,
    /// The largest changed x coordinate.
    pub max_x: u32,
    /// The largest changed y coordinate.
    pub max_y: u32,
}

/// Result of [`compare_rgba`].
///
/// For equal dimensions, `compared_pixels` equals `width * height`,
/// `changed_pixels` counts pixels with any unequal byte, `bounds` holds the
/// smallest rectangle around the changed pixels (`None` when no pixel
/// changed), and `difference_image` holds RGBA8 bytes with the input
/// dimensions.
///
/// For mismatched dimensions, no pixel comparison runs: `compared_pixels` is
/// zero, `changed_pixels` is the larger input pixel count (always nonzero, so
/// the result never reads as a zero-difference success), `bounds` is `None`,
/// and `difference_image` is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PixelComparison {
    /// The number of changed pixels, or the larger input pixel count after a
    /// dimension mismatch.
    pub changed_pixels: u64,
    /// The number of compared pixels, or zero after a dimension mismatch.
    pub compared_pixels: u64,
    /// The smallest rectangle around the changed pixels, if any.
    pub bounds: Option<PixelBounds>,
    /// Location map of the changes, if the dimensions match.
    pub difference_image: Option<Vec<u8>>,
}

impl PixelComparison {
    /// Returns true only when every compared pixel is equal.
    ///
    /// A dimension mismatch returns false.
    #[must_use]
    pub fn is_equal(&self) -> bool {
        self.changed_pixels == 0 && self.difference_image.is_some()
    }
}

/// Compares two decoded RGBA8 images byte by byte.
///
/// Each slice must hold exactly `width * height * 4` bytes in RGBA channel
/// order with straight alpha and sRGB channel bytes. All four bytes of each
/// pixel must match, even when alpha is zero.
///
/// The returned difference image uses fixed magenta (`[255, 0, 255, 255]`)
/// for changed pixels and transparent black (`[0, 0, 0, 0]`) for equal
/// pixels. That image shows change locations. It does not show the numerical
/// magnitude of the changes.
///
/// # Errors
///
/// Returns [`PixelError`] when dimensions are empty, when a slice length is
/// wrong, or when checked arithmetic rejects the dimensions.
///
/// # Example
///
/// ```rust
/// use tui_test::pixel::compare_rgba;
///
/// let first = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
/// let second = vec![10u8, 20, 30, 255, 40, 50, 61, 255];
/// let result = compare_rgba(2, 1, &first, 2, 1, &second).unwrap();
/// assert!(!result.is_equal());
/// assert_eq!(result.changed_pixels, 1);
/// ```
pub fn compare_rgba(
    width_a: u32,
    height_a: u32,
    bytes_a: &[u8],
    width_b: u32,
    height_b: u32,
    bytes_b: &[u8],
) -> Result<PixelComparison, PixelError> {
    let count_a = validated_pixel_count(width_a, height_a, bytes_a, PixelInput::First)?;
    let count_b = validated_pixel_count(width_b, height_b, bytes_b, PixelInput::Second)?;

    if width_a != width_b || height_a != height_b {
        return Ok(PixelComparison {
            changed_pixels: count_a.max(count_b),
            compared_pixels: 0,
            bounds: None,
            difference_image: None,
        });
    }

    let mut changed_pixels: u64 = 0;
    let mut bounds: Option<PixelBounds> = None;
    let mut difference_image: Vec<u8> = Vec::with_capacity(bytes_a.len());
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    let (rows_a, _) = bytes_a.as_chunks::<4>();
    let (rows_b, _) = bytes_b.as_chunks::<4>();
    for (pixel_a, pixel_b) in rows_a.iter().zip(rows_b.iter()) {
        if pixel_a != pixel_b {
            changed_pixels += 1;
            difference_image.extend_from_slice(&[255, 0, 255, 255]);
            bounds = Some(match bounds {
                None => PixelBounds {
                    min_x: x,
                    min_y: y,
                    max_x: x,
                    max_y: y,
                },
                Some(current) => PixelBounds {
                    min_x: current.min_x.min(x),
                    min_y: current.min_y.min(y),
                    max_x: current.max_x.max(x),
                    max_y: current.max_y.max(y),
                },
            });
        } else {
            difference_image.extend_from_slice(&[0, 0, 0, 0]);
        }
        x += 1;
        if x == width_a {
            x = 0;
            y += 1;
        }
    }

    Ok(PixelComparison {
        changed_pixels,
        compared_pixels: count_a,
        bounds,
        difference_image: Some(difference_image),
    })
}

/// Checks dimensions with checked arithmetic and validates the slice length.
///
/// Returns the pixel count. Both dimensions are nonzero here, so each valid
/// image holds at least one pixel.
fn validated_pixel_count(
    width: u32,
    height: u32,
    bytes: &[u8],
    image: PixelInput,
) -> Result<u64, PixelError> {
    if width == 0 || height == 0 {
        return Err(PixelError::EmptyDimensions {
            image,
            width,
            height,
        });
    }
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(usize::try_from(height).ok()?))
        .ok_or(PixelError::PixelCountOverflow {
            image,
            width,
            height,
        })?;
    let expected = pixels
        .checked_mul(4)
        .ok_or(PixelError::PixelCountOverflow {
            image,
            width,
            height,
        })?;
    if bytes.len() != expected {
        return Err(PixelError::LengthMismatch {
            image,
            width,
            height,
            expected,
            actual: bytes.len(),
        });
    }
    Ok(pixels as u64)
}
