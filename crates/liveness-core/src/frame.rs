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
        let invalid = |msg: &str| LivenessError::InvalidFrame(msg.into());
        let (w, h, s) = (
            self.width as usize,
            self.height as usize,
            self.stride as usize,
        );
        let (min_stride, rows) = match self.format {
            PixelFormat::Rgb8 => (w.checked_mul(3), h),
            PixelFormat::Rgba8 | PixelFormat::Bgra8 => (w.checked_mul(4), h),
            // Y plane: h rows of w bytes, then an interleaved VU plane of
            // ceil(h/2) rows of 2 * ceil(w/2) bytes, both using the same stride.
            PixelFormat::Nv21 => (Some(w.div_ceil(2) * 2), h + h.div_ceil(2)),
        };
        let min_stride = min_stride.ok_or_else(|| invalid("frame too large"))?;
        if s < min_stride {
            return Err(invalid("stride too small"));
        }
        let needed = s
            .checked_mul(rows)
            .ok_or_else(|| invalid("frame too large"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(data: &[u8], width: u32, height: u32, stride: u32, format: PixelFormat) -> Frame<'_> {
        Frame {
            data,
            width,
            height,
            stride,
            format,
            rotation: Rotation::Deg0,
            mirrored: false,
            timestamp_ms: 0,
        }
    }

    #[test]
    fn rgb_needs_stride_times_height() {
        let buf = vec![0; 4 * 2 * 3];
        assert!(frame(&buf, 4, 2, 12, PixelFormat::Rgb8).validate().is_ok());
        assert!(frame(&buf, 4, 2, 11, PixelFormat::Rgb8).validate().is_err());
        assert!(
            frame(&buf, 4, 2, 16, PixelFormat::Rgba8)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn nv21_even_size() {
        // 4x4: 16 bytes Y + 8 bytes VU
        assert!(
            frame(&[0; 24], 4, 4, 4, PixelFormat::Nv21)
                .validate()
                .is_ok()
        );
        assert!(
            frame(&[0; 23], 4, 4, 4, PixelFormat::Nv21)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn nv21_odd_size_rounds_chroma_up() {
        // 3x3 needs a stride of 4 (VU row is 2 * ceil(3/2) bytes) and 3 + 2 rows
        assert!(
            frame(&[0; 20], 3, 3, 3, PixelFormat::Nv21)
                .validate()
                .is_err()
        );
        assert!(
            frame(&[0; 19], 3, 3, 4, PixelFormat::Nv21)
                .validate()
                .is_err()
        );
        assert!(
            frame(&[0; 20], 3, 3, 4, PixelFormat::Nv21)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn nv21_respects_row_padding() {
        // 4x4 with 8-byte stride: (4 + 2) rows * 8
        assert!(
            frame(&[0; 24], 4, 4, 8, PixelFormat::Nv21)
                .validate()
                .is_err()
        );
        assert!(
            frame(&[0; 48], 4, 4, 8, PixelFormat::Nv21)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn rejects_zero_size() {
        assert!(
            frame(&[0; 4], 0, 1, 4, PixelFormat::Rgba8)
                .validate()
                .is_err()
        );
    }
}
