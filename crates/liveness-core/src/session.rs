use crate::actions::{
    BlinkConfig, BlinkDetector, HeadTurnDetector, MouthConfig, MouthOpenDetector, TurnConfig,
};
use crate::challenge::{Challenge, ChallengeSession, FailReason, SessionConfig, Status};
use crate::error::Result;
use crate::signals::FaceSignals;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LivenessConfig {
    pub session: SessionConfig,
    pub blink: BlinkConfig,
    pub mouth: MouthConfig,
    pub turn: TurnConfig,
    /// If set, a session that completes its challenges still fails with
    /// [`FailReason::Spoof`] when the mean passive anti-spoofing score (see
    /// [`record_real_score`](LivenessSession::record_real_score)) is below this, or was never
    /// measured. Off by default: calibrate it on your own cameras first.
    pub min_real_score: Option<f32>,
}

/// Runs the action detectors on every frame and feeds what they see into a
/// [`ChallengeSession`], failing the session if the face is lost or a second face appears.
#[derive(Debug, Clone)]
pub struct LivenessSession {
    challenges: ChallengeSession,
    config: SessionConfig,
    blink: BlinkDetector,
    mouth: MouthOpenDetector,
    turn: HeadTurnDetector,
    last_face_ms: Option<u64>,
    multiple_faces_since: Option<u64>,
    min_real_score: Option<f32>,
    real_score_sum: f64,
    real_score_count: u32,
    spoof_failed: bool,
}

impl LivenessSession {
    pub fn new(challenges: Vec<Challenge>, config: LivenessConfig) -> Result<Self> {
        Ok(Self::from_challenges(
            ChallengeSession::new(challenges, config.session)?,
            config,
        ))
    }

    /// See [`ChallengeSession::random`]: `seed` must be unpredictable to the user.
    pub fn random(count: usize, seed: u64, config: LivenessConfig) -> Result<Self> {
        Ok(Self::from_challenges(
            ChallengeSession::random(count, seed, config.session)?,
            config,
        ))
    }

    fn from_challenges(challenges: ChallengeSession, config: LivenessConfig) -> Self {
        Self {
            challenges,
            config: config.session,
            blink: BlinkDetector::new(config.blink),
            mouth: MouthOpenDetector::new(config.mouth),
            turn: HeadTurnDetector::new(config.turn),
            last_face_ms: None,
            multiple_faces_since: None,
            min_real_score: config.min_real_score,
            real_score_sum: 0.0,
            real_score_count: 0,
            spoof_failed: false,
        }
    }

    /// Adds one frame's passive anti-spoofing score (probability of a real face).
    pub fn record_real_score(&mut self, score: f32) {
        self.real_score_sum += f64::from(score);
        self.real_score_count += 1;
    }

    /// Mean of the recorded anti-spoofing scores, if any.
    pub fn mean_real_score(&self) -> Option<f32> {
        (self.real_score_count > 0)
            .then(|| (self.real_score_sum / f64::from(self.real_score_count)) as f32)
    }

    pub fn challenges(&self) -> &[Challenge] {
        self.challenges.challenges()
    }

    /// Call once per frame with every face found in it (empty if none).
    pub fn update(&mut self, now_ms: u64, faces: &[FaceSignals]) -> Status {
        let (status, _) = self.update_with_actions(now_ms, faces);
        status
    }

    /// Like [`update`](Self::update), also returning the actions detected in this frame.
    pub fn update_with_actions(
        &mut self,
        now_ms: u64,
        faces: &[FaceSignals],
    ) -> (Status, Vec<Challenge>) {
        let primary = match faces {
            [face] => Some(*face),
            _ => None,
        };
        self.observe(now_ms, faces.len(), primary)
    }

    /// For callers that only measure one face: `face_count` faces are in view and
    /// `primary` holds the measurements of the main one, if it could be measured.
    /// Returns the status and the actions detected in this frame.
    pub fn observe(
        &mut self,
        now_ms: u64,
        face_count: usize,
        primary: Option<FaceSignals>,
    ) -> (Status, Vec<Challenge>) {
        if self.spoof_failed {
            return (Status::Failed(FailReason::Spoof), vec![]);
        }
        // The face-lost clock only starts once a face has been seen; before that,
        // only the session timeouts apply.
        if let Some(last) = self.last_face_ms
            && now_ms.saturating_sub(last) > self.config.face_lost_ms
        {
            return (self.challenges.fail(FailReason::FaceLost), vec![]);
        }

        if face_count > 0 || primary.is_some() {
            self.last_face_ms = Some(now_ms);
        }
        if face_count > 1 {
            let since = *self.multiple_faces_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= self.config.multiple_faces_ms {
                return (self.challenges.fail(FailReason::MultipleFaces), vec![]);
            }
        } else {
            self.multiple_faces_since = None;
        }

        let mut detected = vec![];
        // With several faces we can't tell whose actions we'd be measuring.
        if let (0 | 1, Some(face)) = (face_count, primary) {
            if self.blink.update(face.ear, now_ms) {
                detected.push(Challenge::Blink);
            }
            if self.mouth.update(face.mouth, now_ms) {
                detected.push(Challenge::OpenMouth);
            }
            if let Some(turn) = self.turn.update(face.yaw, now_ms) {
                detected.push(turn);
            }
        }
        let status = self.challenges.update(now_ms, &detected);
        if status == Status::Passed
            && let Some(min) = self.min_real_score
            && self.mean_real_score().is_none_or(|mean| mean < min)
        {
            self.spoof_failed = true;
        }
        if self.spoof_failed {
            return (Status::Failed(FailReason::Spoof), detected);
        }
        (status, detected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: FaceSignals = FaceSignals {
        ear: 0.3,
        mouth: 0.05,
        yaw: 0.55,
    };

    fn session() -> LivenessSession {
        LivenessSession::new(vec![Challenge::OpenMouth], LivenessConfig::default()).unwrap()
    }

    #[test]
    fn passes_on_detected_action() {
        let mut s = session();
        let mut t = 0;
        for _ in 0..5 {
            s.update(t, &[OPEN]);
            t += 50;
        }
        let wide = FaceSignals { mouth: 0.6, ..OPEN };
        let mut status = Status::Failed(FailReason::Timeout);
        for _ in 0..10 {
            status = s.update(t, &[wide]);
            t += 50;
        }
        assert_eq!(status, Status::Passed);
    }

    fn open_mouth_session(min_real_score: Option<f32>, scores: &[f32]) -> Status {
        let config = LivenessConfig {
            min_real_score,
            ..LivenessConfig::default()
        };
        let mut s = LivenessSession::new(vec![Challenge::OpenMouth], config).unwrap();
        scores.iter().for_each(|&x| s.record_real_score(x));
        let mut t = 0;
        let mut status = Status::Passed;
        for mouth in [0.05; 5].into_iter().chain([0.6; 10]) {
            status = s.update(t, &[FaceSignals { mouth, ..OPEN }]);
            t += 50;
        }
        // Later frames keep the outcome.
        assert_eq!(s.update(t, &[OPEN]), status);
        status
    }

    #[test]
    fn spoof_threshold_is_off_by_default() {
        assert_eq!(open_mouth_session(None, &[0.1]), Status::Passed);
    }

    #[test]
    fn low_real_score_fails_a_completed_session() {
        assert_eq!(open_mouth_session(Some(0.5), &[0.9, 0.8]), Status::Passed);
        assert_eq!(
            open_mouth_session(Some(0.5), &[0.9, 0.05]),
            Status::Failed(FailReason::Spoof)
        );
        // No score measured at all fails closed.
        assert_eq!(
            open_mouth_session(Some(0.5), &[]),
            Status::Failed(FailReason::Spoof)
        );
    }

    #[test]
    fn fails_when_face_lost_too_long() {
        let mut s = session();
        s.update(0, &[OPEN]);
        assert!(matches!(s.update(800, &[]), Status::InProgress { .. }));
        assert_eq!(s.update(1_200, &[]), Status::Failed(FailReason::FaceLost));
    }

    #[test]
    fn face_gap_is_caught_on_next_face_frame() {
        let mut s = session();
        s.update(0, &[OPEN]);
        assert_eq!(
            s.update(5_000, &[OPEN]),
            Status::Failed(FailReason::FaceLost)
        );
    }

    #[test]
    fn no_face_before_first_detection_is_not_face_lost() {
        let mut s = session();
        assert!(matches!(s.update(0, &[]), Status::InProgress { .. }));
        assert!(matches!(s.update(5_000, &[]), Status::InProgress { .. }));
    }

    #[test]
    fn fails_on_persistent_second_face_but_tolerates_a_flicker() {
        let mut s = session();
        s.update(0, &[OPEN]);
        assert!(matches!(
            s.update(50, &[OPEN, OPEN]),
            Status::InProgress { .. }
        ));
        s.update(100, &[OPEN]);
        s.update(150, &[OPEN, OPEN]);
        assert!(matches!(
            s.update(600, &[OPEN, OPEN]),
            Status::InProgress { .. }
        ));
        assert_eq!(
            s.update(650, &[OPEN, OPEN]),
            Status::Failed(FailReason::MultipleFaces)
        );
    }
}
