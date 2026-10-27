use crate::error::{LivenessError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb8,
    Rgba8,
    Bgra8,
    Nv21, // common Android camera format
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes per row (can be larger than width * bytes_per_pixel)
    pub stride: u32,
    pub format: PixelFormat,
    pub rotation: Rotation,
    pub mirrored: bool,
    pub timestamp_ms: u64,
}

impl Frame<'_> {
    pub fn validate(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(LivenessError::InvalidFrame("zero size".into()));
        }
        let (w, h, s) = (self.width as usize, self.height as usize, self.stride as usize);
        let needed = match self.format {
            PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::Bgra8 => {
                let bpp = if self.format == PixelFormat::Rgb8 { 3 } else { 4 };
                if s < w * bpp {
                    return Err(LivenessError::InvalidFrame("stride too small".into()));
                }
                s * h
            }
            PixelFormat::Nv21 => w * h * 3 / 2,
        };
        if self.data.len() < needed {
            return Err(LivenessError::InvalidFrame(format!(
                "buffer too small: {} < {}",
                self.data.len(),
                needed
            )));
        }
        Ok(())
    }
}
