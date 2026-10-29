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
        }
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
        // The face-lost clock only starts once a face has been seen; before that,
        // only the session timeouts apply.
        if let Some(last) = self.last_face_ms
            && now_ms.saturating_sub(last) > self.config.face_lost_ms
        {
            return (self.challenges.fail(FailReason::FaceLost), vec![]);
        }

        if !faces.is_empty() {
            self.last_face_ms = Some(now_ms);
        }
        if faces.len() > 1 {
            let since = *self.multiple_faces_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= self.config.multiple_faces_ms {
                return (self.challenges.fail(FailReason::MultipleFaces), vec![]);
            }
        } else {
            self.multiple_faces_since = None;
        }

        let mut detected = vec![];
        // With several faces we can't tell whose actions we'd be measuring.
        if let [face] = faces {
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
        (self.challenges.update(now_ms, &detected), detected)
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
