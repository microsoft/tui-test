mod nerd_font;
pub mod svg;

#[cfg(feature = "recording-raster")]
pub mod encode;
#[cfg(feature = "recording-raster")]
mod font;
#[cfg(feature = "recording-raster")]
#[allow(dead_code)] // The full raster renderer consumes these primitives next in the stack.
pub mod raster;
