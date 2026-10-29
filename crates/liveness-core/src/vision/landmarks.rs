//! Face-mesh landmark model (256x256 input, 478 landmarks) and the crop geometry around it,
//! following MediaPipe's face_landmark graphs.

use std::f32::consts::PI;

use super::detector::{Detection, LEFT_EYE, RIGHT_EYE};
use super::image::Roi;
use crate::actions::{Point, eye_aspect_ratio};
use crate::signals::{FaceSignals, mouth_ratio, yaw_ratio};

pub(crate) const INPUT_SIZE: usize = 256;
pub(crate) const NUM_LANDMARKS: usize = 478;

/// The crop is this much larger than the face so the whole mesh fits.
const ROI_SCALE: f32 = 1.5;

// Face-mesh indices, matching prototype/probe.py.
// Per eye: corner, upper, upper, corner, lower, lower.
const RIGHT_EYE_IDX: [usize; 6] = [33, 160, 158, 133, 153, 144];
const LEFT_EYE_IDX: [usize; 6] = [362, 385, 387, 263, 373, 380];
const MOUTH_TOP: usize = 13;
const MOUTH_BOTTOM: usize = 14;
const MOUTH_LEFT: usize = 78;
const MOUTH_RIGHT: usize = 308;
const NOSE: usize = 1;
const FACE_LEFT: usize = 234;
const FACE_RIGHT: usize = 454;
/// Outer eye corners, used to measure head roll for tracking.
const RIGHT_EYE_OUTER: usize = 33;
const LEFT_EYE_OUTER: usize = 263;

/// Angle that rotates the line `from -> to` to horizontal, in MediaPipe's convention.
fn roll(from: (f32, f32), to: (f32, f32)) -> f32 {
    normalize_radians(-(-(to.1 - from.1)).atan2(to.0 - from.0))
}

fn normalize_radians(a: f32) -> f32 {
    a - 2.0 * PI * ((a + PI) / (2.0 * PI)).floor()
}

/// Square, roll-corrected crop around an axis-aligned face box.
fn square_roi(cx: f32, cy: f32, width: f32, height: f32, angle: f32) -> Roi {
    let side = width.max(height) * ROI_SCALE;
    Roi {
        cx,
        cy,
        width: side,
        height: side,
        angle,
    }
}

/// Crop for the landmark model from a fresh detection.
pub(crate) fn roi_from_detection(d: &Detection) -> Roi {
    let angle = roll(d.keypoints[RIGHT_EYE], d.keypoints[LEFT_EYE]);
    square_roi(d.cx, d.cy, d.width, d.height, angle)
}

/// Crop for the next frame from this frame's landmarks (tracking, no detector needed).
pub(crate) fn roi_from_landmarks(points: &[Point]) -> Roi {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let (r, l) = (points[RIGHT_EYE_OUTER], points[LEFT_EYE_OUTER]);
    let angle = roll((r.x, r.y), (l.x, l.y));
    square_roi(
        (min_x + max_x) / 2.0,
        (min_y + max_y) / 2.0,
        max_x - min_x,
        max_y - min_y,
        angle,
    )
}

/// Converts raw model output (x, y, z per landmark in input-tensor pixels) to upright pixels.
pub(crate) fn project(raw: &[f32], roi: &Roi) -> Vec<Point> {
    let size = INPUT_SIZE as f32;
    let (points, _) = raw.as_chunks::<3>();
    points
        .iter()
        .take(NUM_LANDMARKS)
        .map(|&[x, y, _z]| {
            let (x, y) = roi.to_image(x / size, y / size);
            Point { x, y }
        })
        .collect()
}

/// Eye, mouth and head-turn measurements from a landmark set.
pub(crate) fn signals(p: &[Point]) -> FaceSignals {
    let eye = |idx: [usize; 6]| eye_aspect_ratio(idx.map(|i| p[i]));
    FaceSignals {
        ear: (eye(RIGHT_EYE_IDX) + eye(LEFT_EYE_IDX)) / 2.0,
        mouth: mouth_ratio(p[MOUTH_TOP], p[MOUTH_BOTTOM], p[MOUTH_LEFT], p[MOUTH_RIGHT]),
        yaw: yaw_ratio(p[NOSE], p[FACE_LEFT], p[FACE_RIGHT]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_is_zero_for_level_eyes_and_positive_when_tilted_clockwise() {
        assert_eq!(roll((10.0, 50.0), (90.0, 50.0)), 0.0);
        // Subject's left eye (image right) lower: head rolled clockwise on screen.
        let a = roll((0.0, 0.0), (10.0, 10.0));
        assert!((a - PI / 4.0).abs() < 1e-6);
    }

    #[test]
    fn detection_roi_is_square_and_scaled() {
        let d = Detection {
            score: 0.9,
            cx: 100.0,
            cy: 120.0,
            width: 40.0,
            height: 60.0,
            keypoints: [
                (90.0, 110.0),
                (110.0, 110.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
            ],
        };
        let roi = roi_from_detection(&d);
        assert_eq!(
            (roi.cx, roi.cy, roi.width, roi.height, roi.angle),
            (100.0, 120.0, 90.0, 90.0, 0.0)
        );
    }

    #[test]
    fn projection_inverts_crop_geometry() {
        let roi = Roi {
            cx: 50.0,
            cy: 60.0,
            width: 100.0,
            height: 100.0,
            angle: 0.0,
        };
        let raw = [128.0, 128.0, 0.0, 0.0, 256.0, 0.0];
        let p = project(&raw, &roi);
        assert_eq!((p[0].x, p[0].y), (50.0, 60.0));
        assert_eq!((p[1].x, p[1].y), (0.0, 110.0));
    }
}
