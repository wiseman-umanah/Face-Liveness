//! Post-processing for MediaPipe's short-range BlazeFace detector (128x128 input,
//! 896 anchors). Mirrors the settings in MediaPipe's `face_detection_short_range.pbtxt`.

use super::image::Roi;

pub(crate) const INPUT_SIZE: usize = 128;
const NUM_ANCHORS: usize = 896;
const NUM_COORDS: usize = 16;
const NUM_KEYPOINTS: usize = 6;
const SCORE_CLIP: f32 = 100.0;
const NMS_IOU: f32 = 0.3;

/// Keypoint indices in a detection.
pub(crate) const RIGHT_EYE: usize = 0;
pub(crate) const LEFT_EYE: usize = 1;

/// A detected face in upright pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Detection {
    pub score: f32,
    pub cx: f32,
    pub cy: f32,
    pub width: f32,
    pub height: f32,
    /// Subject's right eye, left eye, nose tip, mouth, right ear, left ear.
    pub keypoints: [(f32, f32); NUM_KEYPOINTS],
}

impl Detection {
    fn area(&self) -> f32 {
        self.width * self.height
    }

    fn iou(&self, other: &Detection) -> f32 {
        let overlap = |c1: f32, s1: f32, c2: f32, s2: f32| {
            ((c1 + s1 / 2.0).min(c2 + s2 / 2.0) - (c1 - s1 / 2.0).max(c2 - s2 / 2.0)).max(0.0)
        };
        let inter = overlap(self.cx, self.width, other.cx, other.width)
            * overlap(self.cy, self.height, other.cy, other.height);
        let union = self.area() + other.area() - inter;
        if union <= 0.0 { 0.0 } else { inter / union }
    }
}

/// Anchor centers in 0..1 input coordinates (all anchors are 1x1 for this model).
pub(crate) fn anchors() -> Vec<(f32, f32)> {
    // Layers with strides [8, 16, 16, 16]; consecutive layers with the same stride share a
    // grid, and each layer contributes 2 anchors per cell (aspect ratio 1 + interpolated).
    let mut out = Vec::with_capacity(NUM_ANCHORS);
    for (stride, layers) in [(8, 1), (16, 3)] {
        let grid = INPUT_SIZE / stride;
        for y in 0..grid {
            for x in 0..grid {
                for _ in 0..2 * layers {
                    out.push((
                        (x as f32 + 0.5) / grid as f32,
                        (y as f32 + 0.5) / grid as f32,
                    ));
                }
            }
        }
    }
    out
}

/// Decodes raw model outputs into detections in upright pixels.
///
/// `input` is the region of the upright image the 128x128 input tensor was sampled from.
pub(crate) fn decode(
    regressors: &[f32],
    scores: &[f32],
    anchors: &[(f32, f32)],
    input: &Roi,
    min_score: f32,
) -> Vec<Detection> {
    let scale = INPUT_SIZE as f32;
    let mut found = vec![];
    for (i, &(ax, ay)) in anchors.iter().enumerate() {
        let score = sigmoid(scores[i].clamp(-SCORE_CLIP, SCORE_CLIP));
        if score < min_score {
            continue;
        }
        let r = &regressors[i * NUM_COORDS..(i + 1) * NUM_COORDS];
        let (cx, cy) = input.to_image(r[0] / scale + ax, r[1] / scale + ay);
        let mut keypoints = [(0.0, 0.0); NUM_KEYPOINTS];
        for (k, kp) in keypoints.iter_mut().enumerate() {
            *kp = input.to_image(r[4 + 2 * k] / scale + ax, r[5 + 2 * k] / scale + ay);
        }
        found.push(Detection {
            score,
            cx,
            cy,
            width: r[2] / scale * input.width,
            height: r[3] / scale * input.height,
            keypoints,
        });
    }
    weighted_nms(found)
}

/// MediaPipe's weighted non-max suppression: overlapping detections are merged into a
/// score-weighted average instead of being dropped, which steadies boxes between frames.
fn weighted_nms(mut detections: Vec<Detection>) -> Vec<Detection> {
    detections.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut out = vec![];
    while let Some(best) = detections.first().copied() {
        let (group, rest): (Vec<Detection>, Vec<Detection>) =
            detections.into_iter().partition(|d| best.iou(d) > NMS_IOU);
        detections = rest;

        let total: f32 = group.iter().map(|d| d.score).sum();
        let avg = |f: &dyn Fn(&Detection) -> f32| {
            group.iter().map(|d| f(d) * d.score).sum::<f32>() / total
        };
        let mut merged = Detection {
            score: best.score,
            cx: avg(&|d| d.cx),
            cy: avg(&|d| d.cy),
            width: avg(&|d| d.width),
            height: avg(&|d| d.height),
            keypoints: best.keypoints,
        };
        for k in 0..NUM_KEYPOINTS {
            merged.keypoints[k] = (avg(&|d| d.keypoints[k].0), avg(&|d| d.keypoints[k].1));
        }
        out.push(merged);
    }
    out
}

pub(crate) fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_layout() {
        let a = anchors();
        assert_eq!(a.len(), NUM_ANCHORS);
        assert_eq!(a[0], (0.5 / 16.0, 0.5 / 16.0));
        assert_eq!(a[1], a[0]);
        // First stride-16 anchor comes after the 16x16x2 stride-8 block.
        assert_eq!(a[512], (0.5 / 8.0, 0.5 / 8.0));
        assert_eq!(a[895], (7.5 / 8.0, 7.5 / 8.0));
    }

    fn det(score: f32, cx: f32) -> Detection {
        Detection {
            score,
            cx,
            cy: 10.0,
            width: 10.0,
            height: 10.0,
            keypoints: [(cx, 10.0); 6],
        }
    }

    #[test]
    fn nms_merges_overlaps_and_keeps_separate_faces() {
        let out = weighted_nms(vec![det(0.6, 12.0), det(0.9, 10.0), det(0.8, 100.0)]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].score, 0.9);
        // Score-weighted center of the two overlapping boxes.
        let expected = (0.9 * 10.0 + 0.6 * 12.0) / 1.5;
        assert!((out[0].cx - expected).abs() < 1e-4);
        assert!((out[0].keypoints[0].0 - expected).abs() < 1e-4);
        assert_eq!(out[1].cx, 100.0);
    }

    #[test]
    fn decode_maps_to_image_pixels() {
        let anchors = anchors();
        let mut regressors = vec![0.0; NUM_ANCHORS * NUM_COORDS];
        let mut scores = vec![-10.0; NUM_ANCHORS];
        // Anchor 0 sits at (1/32, 1/32); give it a confident 32x32-input-pixel box.
        scores[0] = 5.0;
        regressors[2] = 32.0;
        regressors[3] = 32.0;
        // Input covers a 256x256 image region centered at (128, 128).
        let input = Roi {
            cx: 128.0,
            cy: 128.0,
            width: 256.0,
            height: 256.0,
            angle: 0.0,
        };
        let out = decode(&regressors, &scores, &anchors, &input, 0.5);
        assert_eq!(out.len(), 1);
        assert!((out[0].cx - 8.0).abs() < 1e-4 && (out[0].cy - 8.0).abs() < 1e-4);
        assert!((out[0].width - 64.0).abs() < 1e-4);
        assert!((out[0].score - sigmoid(5.0)).abs() < 1e-6);
    }
}
