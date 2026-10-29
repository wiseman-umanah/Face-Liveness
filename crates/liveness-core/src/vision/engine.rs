use super::{AnalyzerConfig, FaceAnalysis, FaceAnalyzer};
use crate::challenge::{Challenge, Status};
use crate::error::Result;
use crate::frame::Frame;
use crate::session::{LivenessConfig, LivenessSession};

/// Result of one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameResult {
    pub status: Status,
    /// Actions recognised in this frame.
    pub actions: Vec<Challenge>,
    pub analysis: FaceAnalysis,
}

/// Frames in, liveness status out: face analysis + action detection + challenge flow.
///
/// Mirroring is handled through [`Frame::mirrored`]; leave `LivenessConfig::turn.mirrored`
/// false, since the engine already measures faces in un-mirrored coordinates.
pub struct LivenessEngine {
    analyzer: FaceAnalyzer,
    session: LivenessSession,
}

impl LivenessEngine {
    pub fn new(challenges: Vec<Challenge>, config: LivenessConfig) -> Result<Self> {
        let session = LivenessSession::new(challenges, config)?;
        Ok(Self::from_parts(
            FaceAnalyzer::new(AnalyzerConfig::default())?,
            session,
        ))
    }

    /// See [`ChallengeSession::random`](crate::challenge::ChallengeSession::random):
    /// `seed` must be unpredictable to the user.
    pub fn random(count: usize, seed: u64, config: LivenessConfig) -> Result<Self> {
        let session = LivenessSession::random(count, seed, config)?;
        Ok(Self::from_parts(
            FaceAnalyzer::new(AnalyzerConfig::default())?,
            session,
        ))
    }

    /// Reuses an analyzer (loading models takes a moment) or customises its config.
    pub fn from_parts(mut analyzer: FaceAnalyzer, session: LivenessSession) -> Self {
        analyzer.reset();
        Self { analyzer, session }
    }

    pub fn challenges(&self) -> &[Challenge] {
        self.session.challenges()
    }

    /// Call once per camera frame; `frame.timestamp_ms` drives all timing.
    pub fn process(&mut self, frame: &Frame) -> Result<FrameResult> {
        let analysis = self.analyzer.analyze(frame)?;
        let (status, actions) =
            self.session
                .observe(frame.timestamp_ms, analysis.face_count(), analysis.signals);
        Ok(FrameResult {
            status,
            actions,
            analysis,
        })
    }

    /// Takes the analyzer back, e.g. to start the next session without reloading models.
    pub fn into_analyzer(self) -> FaceAnalyzer {
        self.analyzer
    }
}
