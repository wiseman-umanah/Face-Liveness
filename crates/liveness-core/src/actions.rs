#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

fn dist(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Eye Aspect Ratio from 6 eye landmarks: p1 and p4 are the corners,
/// (p2, p6) and (p3, p5) are the upper/lower lid pairs.
pub fn eye_aspect_ratio(p: [Point; 6]) -> f32 {
    let horizontal = dist(p[0], p[3]);
    if horizontal == 0.0 {
        return 0.0;
    }
    (dist(p[1], p[5]) + dist(p[2], p[4])) / (2.0 * horizontal)
}

/// Detects one complete blink: eye closes, then reopens within a plausible time.
pub struct BlinkDetector {
    closed_since: Option<u64>,
    threshold: f32,
    min_ms: u64,
    max_ms: u64,
}

impl BlinkDetector {
    pub fn new() -> Self {
        // 0.2 is a placeholder; real thresholds vary per person and camera
        Self { closed_since: None, threshold: 0.2, min_ms: 50, max_ms: 600 }
    }

    /// Returns true on the frame where a complete blink is detected.
    pub fn update(&mut self, ear: f32, now_ms: u64) -> bool {
        match (self.closed_since, ear < self.threshold) {
            (None, true) => {
                self.closed_since = Some(now_ms);
                false
            }
            (Some(start), false) => {
                self.closed_since = None;
                let d = now_ms.saturating_sub(start);
                d >= self.min_ms && d <= self.max_ms
            }
            _ => false,
        }
    }
}

impl Default for BlinkDetector {
    fn default() -> Self {
        Self::new()
    }
}
