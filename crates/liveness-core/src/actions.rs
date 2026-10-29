use crate::challenge::Challenge;
use crate::signals::RollingMedian;

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

pub(crate) fn dist(a: Point, b: Point) -> f32 {
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

/// Blink thresholds, all relative to the person's own open-eye EAR (rolling median),
/// because absolute EAR varies a lot between people, cameras and sessions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlinkConfig {
    /// A closure starts when EAR falls below `enter` x baseline.
    pub enter: f32,
    /// It ends when EAR recovers above (`enter` + `exit_gap`) x baseline (hysteresis).
    pub exit_gap: f32,
    /// It only counts as a blink if EAR dipped below `depth` x baseline.
    pub depth: f32,
    /// Closures longer than this are not blinks (eyes held shut, looking down).
    pub max_ms: u64,
    /// Number of recent samples the baseline is taken over.
    pub window: usize,
    /// Samples needed before any blink can be reported.
    pub warmup: usize,
}

impl Default for BlinkConfig {
    fn default() -> Self {
        // Tuned on prototype/ recordings: real blinks dip to <=0.65 x baseline,
        // noise stays >=0.75, so depth 0.7 sits in the gap.
        Self {
            enter: 0.8,
            exit_gap: 0.1,
            depth: 0.7,
            max_ms: 600,
            window: 150,
            warmup: 30,
        }
    }
}

/// Detects one complete blink: eye closes deep enough, then reopens within `max_ms`.
#[derive(Debug, Clone)]
pub struct BlinkDetector {
    config: BlinkConfig,
    baseline: RollingMedian,
    /// (closure start, lowest ratio seen) while the eye is closed.
    closure: Option<(u64, f32)>,
}

impl BlinkDetector {
    pub fn new(config: BlinkConfig) -> Self {
        Self {
            baseline: RollingMedian::new(config.window),
            config,
            closure: None,
        }
    }

    /// Returns true on the frame where a complete blink is detected.
    pub fn update(&mut self, ear: f32, now_ms: u64) -> bool {
        self.baseline.push(ear);
        if self.baseline.len() < self.config.warmup {
            return false;
        }
        let median = self.baseline.median();
        if median <= 0.0 {
            return false;
        }
        let ratio = ear / median;

        match self.closure {
            None => {
                if ratio < self.config.enter {
                    self.closure = Some((now_ms, ratio));
                }
                false
            }
            Some((start, lowest)) => {
                let lowest = lowest.min(ratio);
                if ratio < self.config.enter + self.config.exit_gap {
                    self.closure = Some((start, lowest));
                    return false;
                }
                self.closure = None;
                now_ms.saturating_sub(start) <= self.config.max_ms && lowest < self.config.depth
            }
        }
    }
}

impl Default for BlinkDetector {
    fn default() -> Self {
        Self::new(BlinkConfig::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouthConfig {
    /// Mouth ratio that counts as open. Talking peaks around 0.25-0.34,
    /// a deliberate open mouth at 0.45-0.73 in the recordings.
    pub open: f32,
    /// Mouth must drop below this before another open is reported.
    pub rearm: f32,
    /// How long the mouth must stay open.
    pub hold_ms: u64,
}

impl Default for MouthConfig {
    fn default() -> Self {
        Self {
            open: 0.35,
            rearm: 0.2,
            hold_ms: 300,
        }
    }
}

/// Detects a deliberately held open mouth (not speech).
#[derive(Debug, Clone)]
pub struct MouthOpenDetector {
    config: MouthConfig,
    armed: bool,
    open_since: Option<u64>,
}

impl MouthOpenDetector {
    pub fn new(config: MouthConfig) -> Self {
        // Starts disarmed: the mouth has to be seen closed first.
        Self {
            config,
            armed: false,
            open_since: None,
        }
    }

    /// Returns true once per open-mouth gesture, after it has been held for `hold_ms`.
    pub fn update(&mut self, mouth: f32, now_ms: u64) -> bool {
        if !self.armed {
            self.armed = mouth < self.config.rearm;
            return false;
        }
        if mouth <= self.config.open {
            self.open_since = None;
            return false;
        }
        let since = *self.open_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < self.config.hold_ms {
            return false;
        }
        self.armed = false;
        self.open_since = None;
        true
    }
}

impl Default for MouthOpenDetector {
    fn default() -> Self {
        Self::new(MouthConfig::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnConfig {
    /// Yaw must move this far from the person's baseline (rolling median).
    /// Deliberate turns in the recordings move >=0.4; resting jitter stays <0.2.
    pub threshold: f32,
    /// Yaw must come back within this distance of baseline before the next turn.
    pub rearm: f32,
    /// How long the turn must be held.
    pub hold_ms: u64,
    pub window: usize,
    pub warmup: usize,
    /// Set when the frames are a mirror image (selfie preview), which swaps left and right.
    pub mirrored: bool,
}

impl Default for TurnConfig {
    fn default() -> Self {
        Self {
            threshold: 0.3,
            rearm: 0.15,
            hold_ms: 300,
            window: 150,
            warmup: 30,
            mirrored: false,
        }
    }
}

/// Detects a held head turn and reports its direction from the subject's point of view.
#[derive(Debug, Clone)]
pub struct HeadTurnDetector {
    config: TurnConfig,
    baseline: RollingMedian,
    armed: bool,
    /// (start, nose moved toward image right) while turned past the threshold.
    turn: Option<(u64, bool)>,
}

impl HeadTurnDetector {
    pub fn new(config: TurnConfig) -> Self {
        Self {
            baseline: RollingMedian::new(config.window),
            config,
            armed: false,
            turn: None,
        }
    }

    /// Returns `TurnLeft`/`TurnRight` once per turn, after it has been held for `hold_ms`.
    pub fn update(&mut self, yaw: f32, now_ms: u64) -> Option<Challenge> {
        self.baseline.push(yaw);
        if self.baseline.len() < self.config.warmup {
            return None;
        }
        let deviation = yaw - self.baseline.median();

        if !self.armed {
            self.armed = deviation.abs() < self.config.rearm;
            return None;
        }
        if deviation.abs() <= self.config.threshold {
            self.turn = None;
            return None;
        }
        let toward_right = deviation > 0.0;
        let start = match self.turn {
            Some((start, dir)) if dir == toward_right => start,
            _ => {
                self.turn = Some((now_ms, toward_right));
                now_ms
            }
        };
        if now_ms.saturating_sub(start) < self.config.hold_ms {
            return None;
        }
        self.armed = false;
        self.turn = None;
        // In an unmirrored camera image the subject's left is on the image's right.
        Some(if toward_right != self.config.mirrored {
            Challenge::TurnLeft
        } else {
            Challenge::TurnRight
        })
    }
}

impl Default for HeadTurnDetector {
    fn default() -> Self {
        Self::new(TurnConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `values` at 50 ms intervals starting at `t0`; returns the timestamps that fired.
    fn feed(mut f: impl FnMut(f32, u64) -> bool, t0: u64, values: &[f32]) -> Vec<u64> {
        let mut fired = vec![];
        for (i, &v) in values.iter().enumerate() {
            let t = t0 + i as u64 * 50;
            if f(v, t) {
                fired.push(t);
            }
        }
        fired
    }

    fn open_eyes(n: usize) -> Vec<f32> {
        vec![0.3; n]
    }

    #[test]
    fn blink_detected_after_warmup() {
        let mut d = BlinkDetector::default();
        let mut ears = open_eyes(40);
        ears.extend([0.15, 0.12, 0.3, 0.3]);
        assert_eq!(feed(|e, t| d.update(e, t), 0, &ears).len(), 1);
    }

    #[test]
    fn shallow_dip_is_not_a_blink() {
        let mut d = BlinkDetector::default();
        let mut ears = open_eyes(40);
        ears.extend([0.23, 0.22, 0.3, 0.3]); // ~0.75 x baseline, above depth 0.7
        assert!(feed(|e, t| d.update(e, t), 0, &ears).is_empty());
    }

    #[test]
    fn long_closure_is_not_a_blink() {
        let mut d = BlinkDetector::default();
        let mut ears = open_eyes(40);
        ears.extend(vec![0.1; 20]); // 1 s closed
        ears.extend([0.3, 0.3]);
        assert!(feed(|e, t| d.update(e, t), 0, &ears).is_empty());
    }

    #[test]
    fn hysteresis_ignores_flicker_around_enter() {
        let mut d = BlinkDetector::default();
        let mut ears = open_eyes(40);
        // dips, comes back to just above enter (0.8) but below enter + gap (0.9), dips again
        ears.extend([0.15, 0.25, 0.15, 0.3]);
        assert_eq!(feed(|e, t| d.update(e, t), 0, &ears).len(), 1);
    }

    #[test]
    fn mouth_needs_hold_and_ignores_speech() {
        let mut d = MouthOpenDetector::default();
        // closed, short speech spike (100 ms), closed, held open 400 ms
        let m = [
            0.05, 0.05, 0.3, 0.4, 0.05, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5,
        ];
        assert_eq!(feed(|v, t| d.update(v, t), 0, &m), vec![550]);
    }

    #[test]
    fn mouth_fires_once_per_gesture() {
        let mut d = MouthOpenDetector::default();
        let m = [
            0.05, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.05, 0.5, 0.5, 0.5, 0.5, 0.5,
            0.5, 0.5,
        ];
        assert_eq!(feed(|v, t| d.update(v, t), 0, &m).len(), 2);
    }

    #[test]
    fn mouth_open_from_start_is_ignored() {
        let mut d = MouthOpenDetector::default();
        assert!(feed(|v, t| d.update(v, t), 0, &[0.6; 20]).is_empty());
    }

    fn turns(config: TurnConfig, yaws: &[f32]) -> Vec<Challenge> {
        let mut d = HeadTurnDetector::new(config);
        yaws.iter()
            .enumerate()
            .filter_map(|(i, &y)| d.update(y, i as u64 * 50))
            .collect()
    }

    fn turn_sequence(turned: f32) -> Vec<f32> {
        let mut yaws = vec![0.55; 40];
        yaws.extend(vec![turned; 10]); // 450 ms held
        yaws.extend(vec![0.55; 5]);
        yaws
    }

    #[test]
    fn turn_direction_unmirrored_and_mirrored() {
        let yaws = turn_sequence(0.95);
        assert_eq!(
            turns(TurnConfig::default(), &yaws),
            vec![Challenge::TurnLeft]
        );
        let mirrored = TurnConfig {
            mirrored: true,
            ..TurnConfig::default()
        };
        assert_eq!(turns(mirrored, &yaws), vec![Challenge::TurnRight]);
        assert_eq!(
            turns(TurnConfig::default(), &turn_sequence(0.1)),
            vec![Challenge::TurnRight]
        );
    }

    #[test]
    fn brief_glance_is_not_a_turn() {
        let mut yaws = vec![0.55; 40];
        yaws.extend([0.95, 0.95, 0.55, 0.55]); // 100 ms
        assert!(turns(TurnConfig::default(), &yaws).is_empty());
    }

    #[test]
    fn small_head_movement_is_not_a_turn() {
        let mut yaws = vec![0.55; 40];
        yaws.extend(vec![0.75; 20]);
        assert!(turns(TurnConfig::default(), &yaws).is_empty());
    }
}
