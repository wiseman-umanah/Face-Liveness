use crate::error::{LivenessError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Challenge {
    Blink,
    TurnLeft,
    TurnRight,
    OpenMouth,
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
    Timeout,
}

pub struct ChallengeSession {
    challenges: Vec<Challenge>,
    index: usize,
    started_at_ms: Option<u64>,
    timeout_ms: u64,
}

impl ChallengeSession {
    /// Fails on an empty challenge list, which would otherwise pass on the first frame.
    pub fn new(challenges: Vec<Challenge>, timeout_ms: u64) -> Result<Self> {
        if challenges.is_empty() {
            return Err(LivenessError::InvalidConfig(
                "challenge list is empty".into(),
            ));
        }
        Ok(Self {
            challenges,
            index: 0,
            started_at_ms: None,
            timeout_ms,
        })
    }

    /// Call once per frame with whatever actions were detected in that frame.
    pub fn update(&mut self, now_ms: u64, detected: &[Challenge]) -> Status {
        let start = *self.started_at_ms.get_or_insert(now_ms);

        if self.index >= self.challenges.len() {
            return Status::Passed;
        }
        if now_ms.saturating_sub(start) > self.timeout_ms {
            return Status::Failed(FailReason::Timeout);
        }
        if detected.contains(&self.challenges[self.index]) {
            self.index += 1;
            if self.index >= self.challenges.len() {
                return Status::Passed;
            }
        }
        Status::InProgress {
            current: self.challenges[self.index],
            remaining: self.challenges.len() - self.index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_when_completed_in_order() {
        let mut s =
            ChallengeSession::new(vec![Challenge::Blink, Challenge::TurnLeft], 10_000).unwrap();
        assert!(matches!(s.update(0, &[]), Status::InProgress { .. }));
        assert!(matches!(
            s.update(100, &[Challenge::Blink]),
            Status::InProgress { .. }
        ));
        assert_eq!(s.update(200, &[Challenge::TurnLeft]), Status::Passed);
    }

    #[test]
    fn ignores_wrong_action() {
        let mut s = ChallengeSession::new(vec![Challenge::Blink], 10_000).unwrap();
        assert!(matches!(
            s.update(0, &[Challenge::OpenMouth]),
            Status::InProgress { .. }
        ));
    }

    #[test]
    fn rejects_empty_challenge_list() {
        assert!(matches!(
            ChallengeSession::new(vec![], 10_000),
            Err(LivenessError::InvalidConfig(_))
        ));
    }

    #[test]
    fn times_out() {
        let mut s = ChallengeSession::new(vec![Challenge::Blink], 1_000).unwrap();
        s.update(0, &[]);
        assert_eq!(s.update(1_500, &[]), Status::Failed(FailReason::Timeout));
    }
}
