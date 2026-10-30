//! Passive anti-spoofing: minivision's MiniFASNet models (Silent-Face-Anti-Spoofing), which
//! look at image texture around the face for signs of a printed photo or a screen.
//! Preprocessing mirrors their `CropImage` + `AntiSpoofPredict`: an axis-aligned crop of the
//! face box scaled by 2.7x / 4.0x, shifted to stay inside the image, resized to 80x80, BGR,
//! raw 0-255 values, channels-first.

use super::image::{Border, Roi, UprightView};
use crate::actions::Point;

pub(crate) const INPUT_SIZE: usize = 80;
/// Index of the "real face" class in the models' 3-class output.
const REAL_CLASS: usize = 1;

/// MiniFASNetV2, trained on 2.7x face crops.
pub const MODEL_V2: &[u8] = include_bytes!("../../models/antispoof_minifasnet_v2_2.7.onnx");
/// MiniFASNetV1SE, trained on 4.0x face crops.
pub const MODEL_V1SE: &[u8] = include_bytes!("../../models/antispoof_minifasnet_v1se_4.0.onnx");
/// Crop scale each model was trained with, in the order above.
pub(crate) const SCALES: [f32; 2] = [2.7, 4.0];

/// Integer face box (x, y, width, height) like the reference detector produces:
/// the bounding box of the landmarks.
pub(crate) fn face_box(points: &[Point]) -> (i64, i64, i64, i64) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let (left, top) = (min_x as i64, min_y as i64);
    (
        left,
        top,
        (max_x - min_x + 1.0) as i64,
        (max_y - min_y + 1.0) as i64,
    )
}

/// Port of `CropImage._get_new_box`: the scaled box, shifted back inside the image.
/// Returns inclusive pixel bounds (left, top, right, bottom).
pub(crate) fn scaled_box(
    src_w: f32,
    src_h: f32,
    bbox: (i64, i64, i64, i64),
    scale: f32,
) -> (i64, i64, i64, i64) {
    let (x, y, w, h) = (bbox.0 as f32, bbox.1 as f32, bbox.2 as f32, bbox.3 as f32);
    let scale = scale.min((src_h - 1.0) / h).min((src_w - 1.0) / w);
    let (new_w, new_h) = (w * scale, h * scale);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let (mut left, mut top) = (cx - new_w / 2.0, cy - new_h / 2.0);
    let (mut right, mut bottom) = (cx + new_w / 2.0, cy + new_h / 2.0);
    if left < 0.0 {
        right -= left;
        left = 0.0;
    }
    if top < 0.0 {
        bottom -= top;
        top = 0.0;
    }
    if right > src_w - 1.0 {
        left -= right - src_w + 1.0;
        right = src_w - 1.0;
    }
    if bottom > src_h - 1.0 {
        top -= bottom - src_h + 1.0;
        bottom = src_h - 1.0;
    }
    (left as i64, top as i64, right as i64, bottom as i64)
}

/// 1x3x80x80 BGR tensor (0-255) of pixels left..=right, top..=bottom, like
/// `cv2.resize(img[top:bottom+1, left:right+1], (80, 80))`.
pub(crate) fn input(view: &UprightView, bounds: (i64, i64, i64, i64)) -> Vec<f32> {
    let (left, top, right, bottom) = bounds;
    let (w, h) = ((right - left + 1) as f32, (bottom - top + 1) as f32);
    let roi = Roi {
        cx: left as f32 + w / 2.0,
        cy: top as f32 + h / 2.0,
        width: w,
        height: h,
        angle: 0.0,
    };
    let hwc = view.crop(&roi, INPUT_SIZE, (0.0, 255.0), Border::Replicate);
    let plane = INPUT_SIZE * INPUT_SIZE;
    let mut chw = vec![0.0; 3 * plane];
    for (i, &[r, g, b]) in hwc.as_chunks::<3>().0.iter().enumerate() {
        // RGB in, BGR out, rounded like cv2.resize's 8-bit output.
        chw[i] = b.round();
        chw[plane + i] = g.round();
        chw[2 * plane + i] = r.round();
    }
    chw
}

/// Probability of the "real" class from one model's logits.
pub(crate) fn real_probability(logits: &[f32]) -> f32 {
    let max = logits.iter().copied().fold(f32::MIN, f32::max);
    let exp: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
    exp[REAL_CLASS] / exp.iter().sum::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_box_is_shifted_inside_the_image() {
        // 100x100 box centered in a 1000x1000 image, scale 2: no clipping.
        assert_eq!(
            scaled_box(1000.0, 1000.0, (450, 450, 100, 100), 2.0),
            (400, 400, 600, 600)
        );
        // Near the left edge: shifted right instead of cut off.
        assert_eq!(
            scaled_box(1000.0, 1000.0, (0, 450, 100, 100), 2.0),
            (0, 400, 200, 600)
        );
        // Scale limited so the box fits the image.
        // Scale capped at (200 - 1) / 100 = 1.99 so the box fits the image height.
        assert_eq!(
            scaled_box(300.0, 200.0, (100, 50, 100, 100), 4.0),
            (50, 0, 249, 199)
        );
    }

    #[test]
    fn softmax_real_class() {
        assert!((real_probability(&[0.0, 0.0, 0.0]) - 1.0 / 3.0).abs() < 1e-6);
        assert!(real_probability(&[-10.0, 10.0, -10.0]) > 0.999);
        assert!(real_probability(&[1000.0, 0.0, 0.0]) < 1e-6);
    }
}
