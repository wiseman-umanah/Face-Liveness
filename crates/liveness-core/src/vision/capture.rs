//! The face record a liveness session hands to face matching: the best-quality frame of the
//! session, aligned to the standard 5-point template that ArcFace, SFace, MobileFaceNet and
//! most other face-recognition models expect, and encoded as PNG.

use super::image::{Border, UprightView};
use crate::actions::Point;
use crate::signals::FaceSignals;

/// Side of the aligned face crop, the standard face-recognition input size.
pub const DEFAULT_SIZE: u32 = 112;

/// Where the eyes, nose tip and mouth corners go in a 112x112 crop (the ArcFace template, in
/// OpenCV pixel coordinates). Scaled for other sizes.
const TEMPLATE_112: [(f32, f32); 5] = [
    (38.2946, 51.6963), // eye on the image's left (the subject's right)
    (73.5318, 51.5014), // eye on the image's right
    (56.0252, 71.7366), // nose tip
    (41.5493, 92.3655), // mouth corner, image left
    (70.7299, 92.2041), // mouth corner, image right
];

// Face-mesh indices for the template points: iris centers (the pupils, as 5-point face
// detectors report eyes), nose tip and mouth corners.
const IRIS_LEFT: usize = 468;
const IRIS_RIGHT: usize = 473;
const NOSE_TIP: usize = 1;
const MOUTH_LEFT: usize = 61;
const MOUTH_RIGHT: usize = 291;

/// The best face of a session: what to store and compare later.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceCapture {
    /// The aligned face as PNG. This exact byte string is what the receipt's `face_sha256`
    /// covers, so store and send it as-is.
    pub png: Vec<u8>,
    /// The same image as raw RGB, `size * size * 3` bytes, row by row.
    pub rgb: Vec<u8>,
    pub size: u32,
    /// 0-1: how suitable the frame is for matching (frontal, eyes open, mouth closed, sharp,
    /// well lit, big enough). Higher is better; below ~0.3 matching will be unreliable.
    pub quality: f32,
    /// Timestamp of the frame it came from.
    pub timestamp_ms: u64,
}

impl FaceCapture {
    /// Hex SHA-256 of [`png`](Self::png), as recorded in the receipt.
    #[cfg(feature = "attest")]
    pub fn sha256_hex(&self) -> String {
        crate::attest::sha256_hex(&self.png)
    }
}

/// A similarity transform `dst = [[c, -s], [s, c]] * src + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Similarity {
    c: f32,
    s: f32,
    tx: f32,
    ty: f32,
}

impl Similarity {
    /// Least-squares fit (Umeyama, 2D) mapping `src` points onto `dst` points.
    pub(crate) fn fit(src: &[(f32, f32)], dst: &[(f32, f32)]) -> Self {
        let n = src.len() as f32;
        let mean = |pts: &[(f32, f32)]| {
            let (sx, sy) = pts
                .iter()
                .fold((0.0, 0.0), |(ax, ay), (x, y)| (ax + x, ay + y));
            (sx / n, sy / n)
        };
        let (msx, msy) = mean(src);
        let (mdx, mdy) = mean(dst);
        let (mut dot, mut cross, mut norm) = (0.0, 0.0, 0.0);
        for (&(sx, sy), &(dx, dy)) in src.iter().zip(dst) {
            let (ax, ay) = (sx - msx, sy - msy);
            let (bx, by) = (dx - mdx, dy - mdy);
            dot += ax * bx + ay * by;
            cross += ax * by - ay * bx;
            norm += ax * ax + ay * ay;
        }
        let (c, s) = if norm > 0.0 {
            (dot / norm, cross / norm)
        } else {
            (1.0, 0.0)
        };
        Self {
            c,
            s,
            tx: mdx - (c * msx - s * msy),
            ty: mdy - (s * msx + c * msy),
        }
    }

    #[cfg(test)]
    pub(crate) fn apply(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (
            self.c * x - self.s * y + self.tx,
            self.s * x + self.c * y + self.ty,
        )
    }

    /// The inverse mapping, `dst -> src`.
    pub(crate) fn invert(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let det = self.c * self.c + self.s * self.s;
        let (x, y) = (x - self.tx, y - self.ty);
        (
            (self.c * x + self.s * y) / det,
            (-self.s * x + self.c * y) / det,
        )
    }

    /// How much the source is magnified (>1) or shrunk (<1) to make the crop.
    pub(crate) fn scale(&self) -> f32 {
        (self.c * self.c + self.s * self.s).sqrt()
    }
}

/// The 5 template points of a face, in upright image coordinates.
pub(crate) fn five_points(points: &[Point]) -> [(f32, f32); 5] {
    let p = |i: usize| (points[i].x, points[i].y);
    [
        p(IRIS_LEFT),
        p(IRIS_RIGHT),
        p(NOSE_TIP),
        p(MOUTH_LEFT),
        p(MOUTH_RIGHT),
    ]
}

/// The template for a `size` x `size` crop, in continuous coordinates (pixel centers at +0.5).
fn template(size: u32) -> [(f32, f32); 5] {
    let k = size as f32 / 112.0;
    TEMPLATE_112.map(|(x, y)| (x * k + 0.5, y * k + 0.5))
}

/// Aligned RGB crop, plus how much the face had to be magnified to fill it.
pub(crate) fn align(view: &UprightView, points: &[Point], size: u32) -> (Vec<u8>, f32) {
    let to_crop = Similarity::fit(&five_points(points), &template(size));
    let mut rgb = Vec::with_capacity((size * size * 3) as usize);
    for row in 0..size {
        for col in 0..size {
            let (u, v) = to_crop.invert((col as f32 + 0.5, row as f32 + 0.5));
            rgb.extend(
                view.sample(u, v, Border::Replicate)
                    .map(|c| c.round().clamp(0.0, 255.0) as u8),
            );
        }
    }
    (rgb, to_crop.scale())
}

/// 1 at or below `good`, falling linearly to 0 at `bad` (works in either direction).
fn ramp(value: f32, good: f32, bad: f32) -> f32 {
    ((value - bad) / (good - bad)).clamp(0.0, 1.0)
}

/// Quality of an aligned crop for matching, 0-1 (product of independent factors).
pub(crate) fn quality(
    signals: &FaceSignals,
    points: &[Point],
    rgb: &[u8],
    size: u32,
    magnification: f32,
) -> f32 {
    let [eye_l, eye_r, ..] = five_points(points);
    let roll = (eye_r.1 - eye_l.1).atan2(eye_r.0 - eye_l.0).abs();

    let frontal = ramp((signals.yaw - 0.5).abs(), 0.03, 0.2) * ramp(roll, 0.05, 0.35);
    let eyes_open = ramp(signals.ear, 0.2, 0.12);
    let mouth_closed = ramp(signals.mouth, 0.1, 0.35);
    // Upscaling a small face blurs it; no penalty until it is magnified 1.5x.
    let resolution = ramp(magnification, 1.5, 4.0);

    let gray: Vec<f32> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[r, g, b]| 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
        .collect();
    let mean = gray.iter().sum::<f32>() / gray.len() as f32;
    let exposure = ramp((mean - 128.0).abs(), 60.0, 110.0);
    let sharpness = ramp(laplacian_variance(&gray, size as usize), 150.0, 10.0);

    frontal * eyes_open * mouth_closed * resolution * exposure * sharpness
}

/// Variance of the 4-neighbour Laplacian: the standard focus measure (low = blurry).
fn laplacian_variance(gray: &[f32], size: usize) -> f32 {
    let mut values = Vec::with_capacity((size - 2) * (size - 2));
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let at = |dx: isize, dy: isize| {
                gray[(y as isize + dy) as usize * size + (x as isize + dx) as usize]
            };
            values.push(at(-1, 0) + at(1, 0) + at(0, -1) + at(0, 1) - 4.0 * at(0, 0));
        }
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32
}

/// Lossless PNG; deterministic for the same pixels, so its hash can be signed.
pub(crate) fn encode_png(rgb: &[u8], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size, size);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    let mut writer = encoder.write_header().expect("PNG header into memory");
    writer.write_image_data(rgb).expect("PNG data into memory");
    writer.finish().expect("PNG end into memory");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_recovers_a_known_transform() {
        let truth = Similarity {
            c: 0.8,
            s: 0.3,
            tx: 12.0,
            ty: -4.0,
        };
        let src = [
            (0.0, 0.0),
            (10.0, 0.0),
            (5.0, 8.0),
            (2.0, 15.0),
            (9.0, 14.0),
        ];
        let dst = src.map(|p| truth.apply(p));
        let fit = Similarity::fit(&src, &dst);
        for (s, d) in src.iter().zip(dst) {
            let (x, y) = fit.apply(*s);
            assert!((x - d.0).abs() < 1e-4 && (y - d.1).abs() < 1e-4);
            let (bx, by) = fit.invert(d);
            assert!((bx - s.0).abs() < 1e-3 && (by - s.1).abs() < 1e-3);
        }
        assert!((fit.scale() - (0.8f32.powi(2) + 0.3f32.powi(2)).sqrt()).abs() < 1e-5);
    }

    #[test]
    fn ramp_both_directions() {
        assert_eq!(ramp(0.0, 0.03, 0.2), 1.0);
        assert_eq!(ramp(0.3, 0.03, 0.2), 0.0);
        assert_eq!(ramp(0.25, 0.2, 0.12), 1.0);
        assert!((ramp(0.16, 0.2, 0.12) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn flat_image_is_not_sharp() {
        let flat = vec![100.0; 16 * 16];
        assert_eq!(laplacian_variance(&flat, 16), 0.0);
        let checker: Vec<f32> = (0..256)
            .map(|i| {
                if (i / 16 + i % 16) % 2 == 0 {
                    0.0
                } else {
                    255.0
                }
            })
            .collect();
        assert!(laplacian_variance(&checker, 16) > 1000.0);
    }

    #[test]
    fn png_is_deterministic_and_lossless() {
        let rgb: Vec<u8> = (0..8 * 8 * 3).map(|i| (i * 7 % 256) as u8).collect();
        let a = encode_png(&rgb, 8);
        assert_eq!(a, encode_png(&rgb, 8));
        assert_eq!(&a[1..4], b"PNG");
        let decoder = png::Decoder::new(std::io::Cursor::new(&a));
        let mut reader = decoder.read_info().unwrap();
        let mut back = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut back).unwrap();
        assert_eq!(back, rgb);
    }
}
