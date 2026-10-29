//! Reading pixels from a [`Frame`] in "upright" coordinates and cropping model inputs.
//!
//! Upright coordinates are pixels of the frame after applying its `rotation` and undoing
//! `mirrored`, i.e. the scene as a non-mirrored camera held upright would see it.
//! They are continuous: pixel (i, j) covers [i, i+1) x [j, j+1), so its center is (i+0.5, j+0.5).

use crate::frame::{Frame, PixelFormat, Rotation};

/// An oriented rectangle in upright pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Roi {
    pub cx: f32,
    pub cy: f32,
    pub width: f32,
    pub height: f32,
    /// Clockwise rotation in radians (y points down).
    pub angle: f32,
}

impl Roi {
    /// Maps a point given relative to the ROI (0..1 across, 0..1 down, in the ROI's own
    /// rotated axes) to upright pixel coordinates.
    pub fn to_image(self, nx: f32, ny: f32) -> (f32, f32) {
        let dx = (nx - 0.5) * self.width;
        let dy = (ny - 0.5) * self.height;
        let (sin, cos) = self.angle.sin_cos();
        (self.cx + dx * cos - dy * sin, self.cy + dx * sin + dy * cos)
    }
}

/// What to sample outside the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Border {
    /// Black, as MediaPipe pads the face-detector letterbox.
    Zero,
    /// Repeat the nearest edge pixel, as MediaPipe does for landmark crops.
    Replicate,
}

pub(crate) struct UprightView<'a> {
    frame: &'a Frame<'a>,
    pub width: f32,
    pub height: f32,
}

impl<'a> UprightView<'a> {
    pub fn new(frame: &'a Frame<'a>) -> Self {
        let (w, h) = (frame.width as f32, frame.height as f32);
        let (width, height) = match frame.rotation {
            Rotation::Deg0 | Rotation::Deg180 => (w, h),
            Rotation::Deg90 | Rotation::Deg270 => (h, w),
        };
        Self {
            frame,
            width,
            height,
        }
    }

    /// Upright coordinates -> continuous coordinates in the frame's buffer.
    ///
    /// `rotation` is how far the buffer must be turned clockwise to look upright.
    fn to_buffer(&self, u: f32, v: f32) -> (f32, f32) {
        let u = if self.frame.mirrored {
            self.width - u
        } else {
            u
        };
        let (w, h) = (self.frame.width as f32, self.frame.height as f32);
        match self.frame.rotation {
            Rotation::Deg0 => (u, v),
            Rotation::Deg90 => (v, h - u),
            Rotation::Deg180 => (w - u, h - v),
            Rotation::Deg270 => (w - v, u),
        }
    }

    /// Bilinear RGB sample (0..255) at a continuous upright coordinate.
    pub fn sample(&self, u: f32, v: f32, border: Border) -> [f32; 3] {
        let (x, y) = self.to_buffer(u, v);
        let (w, h) = (self.frame.width as f32, self.frame.height as f32);
        if border == Border::Zero && (x < 0.0 || y < 0.0 || x >= w || y >= h) {
            return [0.0; 3];
        }
        // Shift so integer coordinates land on pixel centers.
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let max_x = self.frame.width as i64 - 1;
        let max_y = self.frame.height as i64 - 1;
        let xi = |d: i64| (x0 as i64 + d).clamp(0, max_x) as usize;
        let yi = |d: i64| (y0 as i64 + d).clamp(0, max_y) as usize;

        let p00 = pixel(self.frame, xi(0), yi(0));
        let p10 = pixel(self.frame, xi(1), yi(0));
        let p01 = pixel(self.frame, xi(0), yi(1));
        let p11 = pixel(self.frame, xi(1), yi(1));
        let mut out = [0.0; 3];
        for c in 0..3 {
            let top = p00[c] + (p10[c] - p00[c]) * tx;
            let bottom = p01[c] + (p11[c] - p01[c]) * tx;
            out[c] = top + (bottom - top) * ty;
        }
        out
    }

    /// Samples `roi` into a `size` x `size` NHWC RGB tensor with values scaled to `range`.
    pub fn crop(&self, roi: &Roi, size: usize, range: (f32, f32), border: Border) -> Vec<f32> {
        let scale = (range.1 - range.0) / 255.0;
        let mut out = Vec::with_capacity(size * size * 3);
        for row in 0..size {
            let ny = (row as f32 + 0.5) / size as f32;
            for col in 0..size {
                let nx = (col as f32 + 0.5) / size as f32;
                let (u, v) = roi.to_image(nx, ny);
                out.extend(self.sample(u, v, border).map(|c| range.0 + c * scale));
            }
        }
        out
    }
}

/// RGB (0..255) of buffer pixel (x, y); the caller keeps x, y in bounds.
fn pixel(frame: &Frame, x: usize, y: usize) -> [f32; 3] {
    let stride = frame.stride as usize;
    let d = frame.data;
    match frame.format {
        PixelFormat::Rgb8 => {
            let i = y * stride + x * 3;
            [d[i] as f32, d[i + 1] as f32, d[i + 2] as f32]
        }
        PixelFormat::Bgr8 => {
            let i = y * stride + x * 3;
            [d[i + 2] as f32, d[i + 1] as f32, d[i] as f32]
        }
        PixelFormat::Rgba8 => {
            let i = y * stride + x * 4;
            [d[i] as f32, d[i + 1] as f32, d[i + 2] as f32]
        }
        PixelFormat::Bgra8 => {
            let i = y * stride + x * 4;
            [d[i + 2] as f32, d[i + 1] as f32, d[i] as f32]
        }
        PixelFormat::Nv21 => {
            let luma = d[y * stride + x] as f32;
            // Interleaved V,U plane after the Y plane, one pair per 2x2 block.
            let chroma = stride * frame.height as usize + (y / 2) * stride + (x / 2) * 2;
            let (v, u) = (d[chroma] as f32 - 128.0, d[chroma + 1] as f32 - 128.0);
            // BT.601 full range, as produced by Android cameras.
            [
                (luma + 1.402 * v).clamp(0.0, 255.0),
                (luma - 0.344_136 * u - 0.714_136 * v).clamp(0.0, 255.0),
                (luma + 1.772 * u).clamp(0.0, 255.0),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        data: &[u8],
        width: u32,
        height: u32,
        rotation: Rotation,
        mirrored: bool,
    ) -> Frame<'_> {
        Frame {
            data,
            width,
            height,
            stride: width * 3,
            format: PixelFormat::Rgb8,
            rotation,
            mirrored,
            timestamp_ms: 0,
        }
    }

    /// 3x2 RGB image whose red channel is the pixel index 0..6.
    fn indexed() -> Vec<u8> {
        (0..6u8).flat_map(|i| [i, 0, 0]).collect()
    }

    fn red_at(view: &UprightView, col: usize, row: usize) -> f32 {
        view.sample(col as f32 + 0.5, row as f32 + 0.5, Border::Replicate)[0]
    }

    /// Reads the whole upright image as rows of red values.
    fn upright(data: &[u8], rotation: Rotation, mirrored: bool) -> Vec<Vec<f32>> {
        let f = frame(data, 3, 2, rotation, mirrored);
        let view = UprightView::new(&f);
        (0..view.height as usize)
            .map(|r| {
                (0..view.width as usize)
                    .map(|c| red_at(&view, c, r))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn rotations_and_mirroring() {
        // Buffer:   0 1 2
        //           3 4 5
        let d = indexed();
        assert_eq!(
            upright(&d, Rotation::Deg0, false),
            vec![vec![0., 1., 2.], vec![3., 4., 5.]]
        );
        // Turned 90 degrees clockwise.
        assert_eq!(
            upright(&d, Rotation::Deg90, false),
            vec![vec![3., 0.], vec![4., 1.], vec![5., 2.]]
        );
        assert_eq!(
            upright(&d, Rotation::Deg180, false),
            vec![vec![5., 4., 3.], vec![2., 1., 0.]]
        );
        assert_eq!(
            upright(&d, Rotation::Deg270, false),
            vec![vec![2., 5.], vec![1., 4.], vec![0., 3.]]
        );
        assert_eq!(
            upright(&d, Rotation::Deg0, true),
            vec![vec![2., 1., 0.], vec![5., 4., 3.]]
        );
    }

    #[test]
    fn bilinear_between_pixels_and_borders() {
        let d = indexed();
        let f = frame(&d, 3, 2, Rotation::Deg0, false);
        let view = UprightView::new(&f);
        // Halfway between pixel 0 and pixel 1.
        assert_eq!(view.sample(1.0, 0.5, Border::Replicate)[0], 0.5);
        assert_eq!(view.sample(-5.0, 0.5, Border::Replicate)[0], 0.0);
        assert_eq!(view.sample(10.0, 1.5, Border::Replicate)[0], 5.0);
        assert_eq!(view.sample(10.0, 1.5, Border::Zero)[0], 0.0);
    }

    #[test]
    fn crop_with_identity_roi_reproduces_image() {
        let d: Vec<u8> = (0..16u8).flat_map(|i| [i * 10, 0, 255]).collect();
        let f = Frame {
            stride: 12,
            ..frame(&d, 4, 4, Rotation::Deg0, false)
        };
        let roi = Roi {
            cx: 2.0,
            cy: 2.0,
            width: 4.0,
            height: 4.0,
            angle: 0.0,
        };
        let t = UprightView::new(&f).crop(&roi, 4, (0.0, 255.0), Border::Replicate);
        let reds: Vec<f32> = t.chunks(3).map(|p| p[0]).collect();
        let expected: Vec<f32> = (0..16).map(|i| (i * 10) as f32).collect();
        assert_eq!(reds, expected);
        assert!(t.chunks(3).all(|p| (p[2] - 255.0).abs() < 1e-3));
    }

    #[test]
    fn roi_rotation_is_clockwise() {
        let roi = Roi {
            cx: 0.0,
            cy: 0.0,
            width: 2.0,
            height: 2.0,
            angle: std::f32::consts::FRAC_PI_2,
        };
        // The ROI's "right" edge midpoint ends up below the center after a 90 degree turn.
        let (x, y) = roi.to_image(1.0, 0.5);
        assert!(x.abs() < 1e-6 && (y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn nv21_gray_and_color() {
        // 2x2 image: Y = 100 everywhere, then one V,U pair.
        let gray = [100, 100, 100, 100, 128, 128];
        let f = Frame {
            format: PixelFormat::Nv21,
            stride: 2,
            ..frame(&gray, 2, 2, Rotation::Deg0, false)
        };
        assert_eq!(pixel(&f, 1, 1), [100.0, 100.0, 100.0]);
        let reddish = [100, 100, 100, 100, 200, 128]; // high V
        let f = Frame {
            data: &reddish,
            ..f
        };
        let [r, g, b] = pixel(&f, 0, 0);
        assert!(r > 190.0 && g < 60.0 && (b - 100.0).abs() < 1e-3);
    }
}
