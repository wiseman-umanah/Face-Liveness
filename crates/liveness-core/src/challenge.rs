use crate::error::{LivenessError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Challenge {
    Blink,
    TurnLeft,
    TurnRight,
    OpenMouth,
}

impl Challenge {
    pub const ALL: [Challenge; 4] = [
        Challenge::Blink,
        Challenge::TurnLeft,
        Challenge::TurnRight,
        Challenge::OpenMouth,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    InProgress {
        current: Challenge,
        remaining: usize,
    },
    Passed,
    Failed(FailReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// The whole session took longer than `total_timeout_ms`.
    Timeout,
    /// The current challenge was not completed within `step_timeout_ms`.
    StepTimeout,
    /// The face disappeared for longer than `face_lost_ms`.
    FaceLost,
    /// More than one face was visible for longer than `multiple_faces_ms`.
    MultipleFaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    pub total_timeout_ms: u64,
    pub step_timeout_ms: u64,
    /// Short detection dropouts are normal; only longer gaps fail the session.
    pub face_lost_ms: u64,
    pub multiple_faces_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            total_timeout_ms: 30_000,
            step_timeout_ms: 10_000,
            face_lost_ms: 1_000,
            multiple_faces_ms: 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChallengeSession {
    challenges: Vec<Challenge>,
    config: SessionConfig,
    index: usize,
    started_at_ms: Option<u64>,
    step_started_at_ms: u64,
    /// Set once the session has passed or failed; it never changes afterwards.
    outcome: Option<Status>,
}

impl ChallengeSession {
    /// Fails on an empty challenge list, which would otherwise pass on the first frame.
    pub fn new(challenges: Vec<Challenge>, config: SessionConfig) -> Result<Self> {
        if challenges.is_empty() {
            return Err(LivenessError::InvalidConfig(
                "challenge list is empty".into(),
            ));
        }
        Ok(Self {
            challenges,
            config,
            index: 0,
            started_at_ms: None,
            step_started_at_ms: 0,
            outcome: None,
        })
    }

    /// `count` challenges in random order, never the same one twice in a row.
    ///
    /// `seed` must be unpredictable to the user (e.g. from the server's CSPRNG);
    /// a guessable order lets an attacker pre-record the answers.
    pub fn random(count: usize, seed: u64, config: SessionConfig) -> Result<Self> {
        let mut rng = SplitMix64(seed);
        let mut challenges: Vec<Challenge> = Vec::with_capacity(count);
        for _ in 0..count {
            let options: Vec<Challenge> = Challenge::ALL
                .into_iter()
                .filter(|c| challenges.last() != Some(c))
                .collect();
            challenges.push(options[(rng.next() % options.len() as u64) as usize]);
        }
        Self::new(challenges, config)
    }

    pub fn challenges(&self) -> &[Challenge] {
        &self.challenges
    }

    /// Ends the session as failed; later updates keep returning this.
    pub fn fail(&mut self, reason: FailReason) -> Status {
        self.outcome.get_or_insert(Status::Failed(reason)).clone()
    }

    /// Call once per frame with whatever actions were detected in that frame.
    pub fn update(&mut self, now_ms: u64, detected: &[Challenge]) -> Status {
        if let Some(outcome) = &self.outcome {
            return outcome.clone();
        }
        let start = *self.started_at_ms.get_or_insert_with(|| {
            self.step_started_at_ms = now_ms;
            now_ms
        });

        if now_ms.saturating_sub(start) > self.config.total_timeout_ms {
            return self.fail(FailReason::Timeout);
        }
        if now_ms.saturating_sub(self.step_started_at_ms) > self.config.step_timeout_ms {
            return self.fail(FailReason::StepTimeout);
        }
        if detected.contains(&self.challenges[self.index]) {
            self.index += 1;
            self.step_started_at_ms = now_ms;
            if self.index == self.challenges.len() {
                self.outcome = Some(Status::Passed);
                return Status::Passed;
            }
        }
        Status::InProgress {
            current: self.challenges[self.index],
            remaining: self.challenges.len() - self.index,
        }
    }
}

/// Tiny dependency-free PRNG, only used to shuffle challenges from a caller-supplied seed.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(challenges: Vec<Challenge>) -> ChallengeSession {
        ChallengeSession::new(challenges, SessionConfig::default()).unwrap()
    }

    #[test]
    fn passes_when_completed_in_order() {
        let mut s = session(vec![Challenge::Blink, Challenge::TurnLeft]);
        assert!(matches!(s.update(0, &[]), Status::InProgress { .. }));
        assert!(matches!(
            s.update(100, &[Challenge::Blink]),
            Status::InProgress { .. }
        ));
        assert_eq!(s.update(200, &[Challenge::TurnLeft]), Status::Passed);
    }

    #[test]
    fn ignores_wrong_action() {
        let mut s = session(vec![Challenge::Blink]);
        assert!(matches!(
            s.update(0, &[Challenge::OpenMouth]),
            Status::InProgress { .. }
        ));
    }

    #[test]
    fn rejects_empty_challenge_list() {
        assert!(matches!(
            ChallengeSession::new(vec![], SessionConfig::default()),
            Err(LivenessError::InvalidConfig(_))
        ));
        assert!(ChallengeSession::random(0, 1, SessionConfig::default()).is_err());
    }

    #[test]
    fn times_out() {
        let config = SessionConfig {
            total_timeout_ms: 1_000,
            ..SessionConfig::default()
        };
        let mut s = ChallengeSession::new(vec![Challenge::Blink], config).unwrap();
        s.update(0, &[]);
        assert_eq!(s.update(1_500, &[]), Status::Failed(FailReason::Timeout));
    }

    #[test]
    fn step_times_out_even_if_total_has_time_left() {
        let config = SessionConfig {
            total_timeout_ms: 10_000,
            step_timeout_ms: 1_000,
            ..SessionConfig::default()
        };
        let mut s =
            ChallengeSession::new(vec![Challenge::Blink, Challenge::OpenMouth], config).unwrap();
        s.update(0, &[]);
        s.update(900, &[Challenge::Blink]); // step 2 starts at 900
        assert!(matches!(s.update(1_800, &[]), Status::InProgress { .. }));
        assert_eq!(
            s.update(2_000, &[]),
            Status::Failed(FailReason::StepTimeout)
        );
    }

    #[test]
    fn outcome_is_sticky() {
        let config = SessionConfig {
            total_timeout_ms: 1_000,
            ..SessionConfig::default()
        };
        let mut s = ChallengeSession::new(vec![Challenge::Blink], config).unwrap();
        s.update(0, &[]);
        s.update(1_500, &[]);
        assert_eq!(
            s.update(1_600, &[Challenge::Blink]),
            Status::Failed(FailReason::Timeout)
        );

        let mut s = session(vec![Challenge::Blink]);
        assert_eq!(s.update(0, &[Challenge::Blink]), Status::Passed);
        assert_eq!(s.fail(FailReason::FaceLost), Status::Passed);
    }

    #[test]
    fn random_is_seeded_and_has_no_repeats() {
        let a = ChallengeSession::random(20, 42, SessionConfig::default()).unwrap();
        let b = ChallengeSession::random(20, 42, SessionConfig::default()).unwrap();
        let c = ChallengeSession::random(20, 43, SessionConfig::default()).unwrap();
        assert_eq!(a.challenges(), b.challenges());
        assert_ne!(a.challenges(), c.challenges());
        assert!(a.challenges().windows(2).all(|w| w[0] != w[1]));
        for kind in Challenge::ALL {
            assert!(a.challenges().contains(&kind), "{kind:?} never chosen");
        }
    }
}
