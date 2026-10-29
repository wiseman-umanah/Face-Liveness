//! Per-frame face measurements: the boundary between "vision" (finding landmarks)
//! and "decision" (detectors and challenges). Detectors only ever see these values.

use std::collections::VecDeque;

use crate::actions::{Point, dist};

/// Measurements for one detected face in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceSignals {
    /// Eye Aspect Ratio averaged over both eyes; drops toward 0 when the eyes close.
    pub ear: f32,
    /// Inner-lip height / width; ~0.05 closed, >0.4 wide open.
    pub mouth: f32,
    /// Nose position between the face edges, 0..1; ~0.5 facing the camera.
    pub yaw: f32,
}

/// Inner-lip opening relative to mouth width.
pub fn mouth_ratio(top: Point, bottom: Point, left: Point, right: Point) -> f32 {
    let horizontal = dist(left, right);
    if horizontal == 0.0 {
        return 0.0;
    }
    dist(top, bottom) / horizontal
}

/// Where the nose sits between the two face edges, along x.
///
/// Clamped to 0..1: on large turns the far face edge is occluded and the raw
/// ratio overshoots (the recordings reach ~1.7), which carries no extra meaning.
pub fn yaw_ratio(nose: Point, face_left: Point, face_right: Point) -> f32 {
    let width = face_right.x - face_left.x;
    if width == 0.0 {
        return 0.5;
    }
    ((nose.x - face_left.x) / width).clamp(0.0, 1.0)
}

/// Median of the most recent `window` samples, used as a per-person baseline.
#[derive(Debug, Clone)]
pub(crate) struct RollingMedian {
    samples: VecDeque<f32>,
    window: usize,
    scratch: Vec<f32>,
}

impl RollingMedian {
    pub(crate) fn new(window: usize) -> Self {
        let window = window.max(1);
        Self {
            samples: VecDeque::with_capacity(window),
            window,
            scratch: Vec::with_capacity(window),
        }
    }

    pub(crate) fn push(&mut self, value: f32) {
        if self.samples.len() == self.window {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }

    /// Matches Python's `statistics.median` (mean of the two middle values for even counts).
    pub(crate) fn median(&mut self) -> f32 {
        self.scratch.clear();
        self.scratch.extend(self.samples.iter().copied());
        self.scratch.sort_unstable_by(f32::total_cmp);
        let n = self.scratch.len();
        match n {
            0 => 0.0,
            _ if n % 2 == 1 => self.scratch[n / 2],
            _ => (self.scratch[n / 2 - 1] + self.scratch[n / 2]) / 2.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn yaw_is_centered_and_clamped() {
        assert_eq!(yaw_ratio(p(50.0, 0.0), p(0.0, 0.0), p(100.0, 0.0)), 0.5);
        assert_eq!(yaw_ratio(p(170.0, 0.0), p(0.0, 0.0), p(100.0, 0.0)), 1.0);
        assert_eq!(yaw_ratio(p(-20.0, 0.0), p(0.0, 0.0), p(100.0, 0.0)), 0.0);
    }

    #[test]
    fn mouth_ratio_handles_zero_width() {
        assert_eq!(
            mouth_ratio(p(0.0, 0.0), p(0.0, 5.0), p(1.0, 1.0), p(1.0, 1.0)),
            0.0
        );
        assert_eq!(
            mouth_ratio(p(0.0, 0.0), p(0.0, 5.0), p(0.0, 2.0), p(10.0, 2.0)),
            0.5
        );
    }

    #[test]
    fn rolling_median_drops_old_samples() {
        let mut m = RollingMedian::new(3);
        for v in [10.0, 1.0, 2.0, 3.0] {
            m.push(v);
        }
        assert_eq!(m.len(), 3);
        assert_eq!(m.median(), 2.0);
        m.push(4.0);
        assert_eq!(m.median(), 3.0);
    }

    #[test]
    fn rolling_median_averages_even_counts() {
        let mut m = RollingMedian::new(4);
        for v in [1.0, 2.0, 3.0, 4.0] {
            m.push(v);
        }
        assert_eq!(m.median(), 2.5);
    }
}
