use super::capture::{self, FaceCapture};
use super::{AnalyzerConfig, FaceAnalysis, FaceAnalyzer};
#[cfg(feature = "attest")]
use crate::attest::{FrameDigest, Receipt, Ticket};
use crate::challenge::{Challenge, Status};
#[cfg(feature = "attest")]
use crate::error::LivenessError;
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
    status: Option<Status>,
    capture_size: u32,
    capture: Option<FaceCapture>,
    #[cfg(feature = "attest")]
    ticket: Option<(Ticket, FrameDigest)>,
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
        Self {
            analyzer,
            session,
            status: None,
            capture_size: capture::DEFAULT_SIZE,
            capture: None,
            #[cfg(feature = "attest")]
            ticket: None,
        }
    }

    /// Side of the face record crop (default 112, the usual face-recognition input; 32-512).
    pub fn set_capture_size(&mut self, size: u32) {
        self.capture_size = size.clamp(32, 512);
    }

    /// The best face seen during the session so far: what to hand to face matching.
    /// Only trust it together with a passed receipt (see
    /// [`PublicKey::verify_face_record`](crate::attest::PublicKey::verify_face_record)).
    pub fn capture(&self) -> Option<&FaceCapture> {
        self.capture.as_ref()
    }

    /// A session for a verified ticket (see [`crate::attest`]): the ticket fixes the
    /// challenges, every frame is hashed, and [`receipt`](Self::receipt) signs off the outcome.
    #[cfg(feature = "attest")]
    pub fn from_ticket(ticket: Ticket, config: LivenessConfig) -> Result<Self> {
        let session = LivenessSession::new(ticket.challenges.clone(), config)?;
        let mut engine = Self::from_parts(FaceAnalyzer::new(AnalyzerConfig::default())?, session);
        let digest = FrameDigest::new(&ticket.nonce);
        engine.ticket = Some((ticket, digest));
        Ok(engine)
    }

    /// The outcome of a finished ticketed session, ready for
    /// [`SigningKey::sign_receipt`](crate::attest::SigningKey::sign_receipt). `now` is Unix
    /// seconds; the receipt is valid for `ttl_secs`.
    #[cfg(feature = "attest")]
    pub fn receipt(&self, now: u64, ttl_secs: u64) -> Result<Receipt> {
        let invalid = |msg: &str| LivenessError::InvalidToken(msg.into());
        let (ticket, digest) = self
            .ticket
            .as_ref()
            .ok_or_else(|| invalid("engine has no ticket"))?;
        let fail_reason = match &self.status {
            Some(Status::Passed) => None,
            Some(Status::Failed(reason)) => Some(*reason),
            _ => return Err(invalid("session is not finished")),
        };
        if now > ticket.expires_at {
            return Err(invalid("ticket expired before the session finished"));
        }
        Ok(Receipt {
            nonce: ticket.nonce.clone(),
            challenges: ticket.challenges.clone(),
            fail_reason,
            frames: digest.frames(),
            frames_sha256: digest.hex(),
            real_score: self.session.mean_real_score(),
            face_sha256: self.capture.as_ref().map(FaceCapture::sha256_hex),
            face_quality: self.capture.as_ref().map(|c| c.quality),
            issued_at: now,
            expires_at: now + ttl_secs,
        })
    }

    pub fn challenges(&self) -> &[Challenge] {
        self.session.challenges()
    }

    /// Call once per camera frame; `frame.timestamp_ms` drives all timing.
    pub fn process(&mut self, frame: &Frame) -> Result<FrameResult> {
        let analysis = self.analyzer.analyze(frame)?;
        if let Some(score) = analysis.real_score {
            self.session.record_real_score(score);
        }
        #[cfg(feature = "attest")]
        if let Some((_, digest)) = &mut self.ticket {
            digest.update(frame);
        }
        let was_running = matches!(self.status, None | Some(Status::InProgress { .. }));
        let (status, actions) =
            self.session
                .observe(frame.timestamp_ms, analysis.face_count(), analysis.signals);
        self.status = Some(status.clone());
        // Keep the best face of the session, up to and including the frame that ends it.
        if was_running && analysis.face_count() == 1 {
            self.consider_capture(frame, &analysis);
        }
        Ok(FrameResult {
            status,
            actions,
            analysis,
        })
    }

    fn consider_capture(&mut self, frame: &Frame, analysis: &FaceAnalysis) {
        let (Some(points), Some(signals)) = (&analysis.landmarks, &analysis.signals) else {
            return;
        };
        let size = self.capture_size;
        let (rgb, quality) = self
            .analyzer
            .capture_candidate(frame, points, signals, size);
        if self
            .capture
            .as_ref()
            .is_none_or(|best| quality > best.quality)
        {
            self.capture = Some(FaceCapture {
                png: capture::encode_png(&rgb, size),
                rgb,
                size,
                quality,
                timestamp_ms: frame.timestamp_ms,
            });
        }
    }

    /// Takes the analyzer back, e.g. to start the next session without reloading models.
    pub fn into_analyzer(self) -> FaceAnalyzer {
        self.analyzer
    }
}

// The bindings run the engine off the main thread (Python without the GIL, Node's thread pool).
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<LivenessEngine>();
};
