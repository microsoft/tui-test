mod nerd_font;
pub mod svg;

#[cfg(feature = "recording-raster")]
#[allow(dead_code)] // The raster primitive layer consumes the font catalog next in the stack.
mod font;
#[cfg(feature = "recording-raster")]
pub mod raster;
