//! Native part of the `faceliveness` Python package. The Python layer in
//! `src/faceliveness/__init__.py` turns NumPy arrays into the raw bytes these functions take.

use std::time::{SystemTime, UNIX_EPOCH};

use liveness_core::attest::{self, PublicKey};
use liveness_core::challenge::{Challenge, Status};
use liveness_core::error::LivenessError;
use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::session::{LivenessConfig, LivenessSession};
use liveness_core::signals::FaceSignals;
use liveness_core::vision::{FaceAnalysis, LivenessEngine};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

fn to_py(e: LivenessError) -> PyErr {
    match e {
        LivenessError::Model(_) => PyRuntimeError::new_err(e.to_string()),
        _ => PyValueError::new_err(e.to_string()),
    }
}

fn parse_challenges(names: Vec<String>) -> PyResult<Vec<Challenge>> {
    names.iter().map(|n| n.parse().map_err(to_py)).collect()
}

fn config(
    total_timeout_ms: Option<u64>,
    step_timeout_ms: Option<u64>,
    face_lost_ms: Option<u64>,
    min_real_score: Option<f32>,
) -> LivenessConfig {
    let mut c = LivenessConfig {
        min_real_score,
        ..LivenessConfig::default()
    };
    if let Some(v) = total_timeout_ms {
        c.session.total_timeout_ms = v;
    }
    if let Some(v) = step_timeout_ms {
        c.session.step_timeout_ms = v;
    }
    if let Some(v) = face_lost_ms {
        c.session.face_lost_ms = v;
    }
    c
}

/// `now` in Unix seconds, defaulting to the system clock.
fn now_or_clock(now: Option<u64>) -> u64 {
    now.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    })
}

fn key32(bytes: &[u8], what: &str) -> PyResult<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| PyValueError::new_err(format!("{what} must be 32 bytes, got {}", bytes.len())))
}

fn public_key(bytes: &[u8]) -> PyResult<PublicKey> {
    PublicKey::from_bytes(&key32(bytes, "public key")?).map_err(to_py)
}

fn seed_or_random(seed: Option<u64>) -> PyResult<u64> {
    match seed {
        Some(s) => Ok(s),
        None => {
            getrandom::u64().map_err(|e| PyRuntimeError::new_err(format!("no OS randomness: {e}")))
        }
    }
}

/// Eye, mouth and head-turn measurements of one face.
#[pyclass(frozen, get_all, module = "faceliveness")]
#[derive(Clone)]
struct Signals {
    /// Eye aspect ratio, ~0.25-0.35 open, drops sharply during a blink.
    ear: f32,
    /// Inner-lip height / width, ~0.05 closed, >0.4 wide open.
    mouth: f32,
    /// Nose position between the face edges, ~0.5 facing the camera.
    yaw: f32,
}

#[pymethods]
impl Signals {
    fn __repr__(&self) -> String {
        format!(
            "Signals(ear={:.3}, mouth={:.3}, yaw={:.3})",
            self.ear, self.mouth, self.yaw
        )
    }
}

impl From<FaceSignals> for Signals {
    fn from(s: FaceSignals) -> Self {
        Self {
            ear: s.ear,
            mouth: s.mouth,
            yaw: s.yaw,
        }
    }
}

/// A detected face box in upright pixel coordinates.
#[pyclass(frozen, get_all, module = "faceliveness")]
#[derive(Clone)]
struct FaceBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    score: f32,
}

#[pymethods]
impl FaceBox {
    fn __repr__(&self) -> String {
        format!(
            "FaceBox(x={:.0}, y={:.0}, width={:.0}, height={:.0}, score={:.2})",
            self.x, self.y, self.width, self.height, self.score
        )
    }
}

/// Outcome of one frame (or one Session.update call).
#[pyclass(frozen, get_all, module = "faceliveness")]
struct FrameResult {
    /// "in_progress", "passed" or "failed".
    status: &'static str,
    /// Challenge the user should do now, while in progress.
    current_challenge: Option<&'static str>,
    /// Challenges left including the current one, while in progress.
    remaining: usize,
    /// "timeout", "step_timeout", "face_lost", "multiple_faces" or "spoof_suspected".
    fail_reason: Option<&'static str>,
    /// Actions recognised in this frame.
    actions: Vec<&'static str>,
    face_count: usize,
    signals: Option<Signals>,
    faces: Vec<FaceBox>,
    /// 478 face-mesh landmarks (x, y) of the primary face, in upright pixels.
    landmarks: Option<Vec<(f32, f32)>>,
    /// Passive anti-spoofing: probability (0-1) that the face is real rather than a photo or
    /// screen. Measured on every 3rd frame with a face (None otherwise); uncalibrated.
    real_score: Option<f32>,
}

#[pymethods]
impl FrameResult {
    /// True once the session has passed or failed; later frames won't change it.
    #[getter]
    fn done(&self) -> bool {
        self.status != "in_progress"
    }

    fn __repr__(&self) -> String {
        let detail = match (self.current_challenge, self.fail_reason) {
            (Some(c), _) => format!(", current_challenge={c:?}, remaining={}", self.remaining),
            (_, Some(r)) => format!(", fail_reason={r:?}"),
            _ => String::new(),
        };
        format!(
            "FrameResult(status={:?}{detail}, face_count={})",
            self.status, self.face_count
        )
    }
}

impl FrameResult {
    fn new(
        status: Status,
        actions: Vec<Challenge>,
        face_count: usize,
        signals: Option<FaceSignals>,
    ) -> Self {
        let (status, current_challenge, remaining, fail_reason) = match status {
            Status::InProgress { current, remaining } => {
                ("in_progress", Some(current.as_str()), remaining, None)
            }
            Status::Passed => ("passed", None, 0, None),
            Status::Failed(reason) => ("failed", None, 0, Some(reason.as_str())),
        };
        Self {
            status,
            current_challenge,
            remaining,
            fail_reason,
            actions: actions.iter().map(|a| a.as_str()).collect(),
            face_count,
            signals: signals.map(Signals::from),
            faces: vec![],
            landmarks: None,
            real_score: None,
        }
    }

    fn with_analysis(mut self, analysis: FaceAnalysis) -> Self {
        self.faces = analysis
            .faces
            .iter()
            .map(|f| FaceBox {
                x: f.x,
                y: f.y,
                width: f.width,
                height: f.height,
                score: f.score,
            })
            .collect();
        self.landmarks = analysis
            .landmarks
            .map(|l| l.iter().map(|p| (p.x, p.y)).collect());
        self.real_score = analysis.real_score;
        self
    }
}

/// Frames in, liveness verdict out (raw-bytes API; see faceliveness.Engine).
#[pyclass(module = "faceliveness._core")]
struct Engine {
    inner: LivenessEngine,
}

#[pymethods]
impl Engine {
    #[new]
    #[pyo3(signature = (challenges, *, total_timeout_ms=None, step_timeout_ms=None, face_lost_ms=None, min_real_score=None))]
    fn new(
        challenges: Vec<String>,
        total_timeout_ms: Option<u64>,
        step_timeout_ms: Option<u64>,
        face_lost_ms: Option<u64>,
        min_real_score: Option<f32>,
    ) -> PyResult<Self> {
        let config = config(
            total_timeout_ms,
            step_timeout_ms,
            face_lost_ms,
            min_real_score,
        );
        let inner = LivenessEngine::new(parse_challenges(challenges)?, config).map_err(to_py)?;
        Ok(Self { inner })
    }

    #[staticmethod]
    #[pyo3(signature = (count, *, seed=None, total_timeout_ms=None, step_timeout_ms=None, face_lost_ms=None, min_real_score=None))]
    fn random(
        count: usize,
        seed: Option<u64>,
        total_timeout_ms: Option<u64>,
        step_timeout_ms: Option<u64>,
        face_lost_ms: Option<u64>,
        min_real_score: Option<f32>,
    ) -> PyResult<Self> {
        let config = config(
            total_timeout_ms,
            step_timeout_ms,
            face_lost_ms,
            min_real_score,
        );
        let inner = LivenessEngine::random(count, seed_or_random(seed)?, config).map_err(to_py)?;
        Ok(Self { inner })
    }

    /// A session for a ticket issued by `SigningKey.issue_ticket`; raises if the ticket is
    /// forged, for another key, or expired.
    #[staticmethod]
    #[pyo3(signature = (token, public_key, *, now=None, total_timeout_ms=None, step_timeout_ms=None, face_lost_ms=None, min_real_score=None))]
    #[allow(clippy::too_many_arguments)]
    fn from_ticket(
        token: &str,
        public_key: &[u8],
        now: Option<u64>,
        total_timeout_ms: Option<u64>,
        step_timeout_ms: Option<u64>,
        face_lost_ms: Option<u64>,
        min_real_score: Option<f32>,
    ) -> PyResult<Self> {
        let ticket = self::public_key(public_key)?
            .verify_ticket(token, now_or_clock(now))
            .map_err(to_py)?;
        let config = config(
            total_timeout_ms,
            step_timeout_ms,
            face_lost_ms,
            min_real_score,
        );
        let inner = LivenessEngine::from_ticket(ticket, config).map_err(to_py)?;
        Ok(Self { inner })
    }

    /// Signed receipt (JWT) for a finished ticketed session.
    #[pyo3(signature = (signing_key, *, ttl_secs=300, now=None))]
    fn receipt(
        &self,
        signing_key: &SigningKey,
        ttl_secs: u64,
        now: Option<u64>,
    ) -> PyResult<String> {
        let receipt = self
            .inner
            .receipt(now_or_clock(now), ttl_secs)
            .map_err(to_py)?;
        signing_key.inner.sign_receipt(&receipt).map_err(to_py)
    }

    #[getter]
    fn challenges(&self) -> Vec<&'static str> {
        self.inner.challenges().iter().map(|c| c.as_str()).collect()
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (data, width, height, stride, format, rotation, mirrored, timestamp_ms))]
    fn process_raw(
        &mut self,
        py: Python<'_>,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        format: &str,
        rotation: u32,
        mirrored: bool,
        timestamp_ms: u64,
    ) -> PyResult<FrameResult> {
        let frame = Frame {
            data,
            width,
            height,
            stride,
            format: format.parse::<PixelFormat>().map_err(to_py)?,
            rotation: Rotation::from_degrees(rotation).map_err(to_py)?,
            mirrored,
            timestamp_ms,
        };
        let engine = &mut self.inner;
        // Inference takes tens of milliseconds; let other Python threads run meanwhile.
        let result = py.detach(|| engine.process(&frame)).map_err(to_py)?;
        let face_count = result.analysis.face_count();
        Ok(FrameResult::new(
            result.status,
            result.actions,
            face_count,
            result.analysis.signals,
        )
        .with_analysis(result.analysis))
    }
}

/// Liveness decisions from measurements you compute yourself (no image processing).
#[pyclass(module = "faceliveness")]
struct Session {
    inner: LivenessSession,
}

#[pymethods]
impl Session {
    #[new]
    #[pyo3(signature = (challenges, *, total_timeout_ms=None, step_timeout_ms=None, face_lost_ms=None))]
    fn new(
        challenges: Vec<String>,
        total_timeout_ms: Option<u64>,
        step_timeout_ms: Option<u64>,
        face_lost_ms: Option<u64>,
    ) -> PyResult<Self> {
        let config = config(total_timeout_ms, step_timeout_ms, face_lost_ms, None);
        let inner = LivenessSession::new(parse_challenges(challenges)?, config).map_err(to_py)?;
        Ok(Self { inner })
    }

    /// `count` random challenges; `seed` defaults to OS randomness.
    #[staticmethod]
    #[pyo3(signature = (count, *, seed=None, total_timeout_ms=None, step_timeout_ms=None, face_lost_ms=None))]
    fn random(
        count: usize,
        seed: Option<u64>,
        total_timeout_ms: Option<u64>,
        step_timeout_ms: Option<u64>,
        face_lost_ms: Option<u64>,
    ) -> PyResult<Self> {
        let config = config(total_timeout_ms, step_timeout_ms, face_lost_ms, None);
        let inner = LivenessSession::random(count, seed_or_random(seed)?, config).map_err(to_py)?;
        Ok(Self { inner })
    }

    #[getter]
    fn challenges(&self) -> Vec<&'static str> {
        self.inner.challenges().iter().map(|c| c.as_str()).collect()
    }

    /// One frame: how many faces are visible, and the main face's measurements if known.
    #[pyo3(signature = (timestamp_ms, *, face_count=1, ear=None, mouth=None, yaw=None))]
    fn update(
        &mut self,
        timestamp_ms: u64,
        face_count: usize,
        ear: Option<f32>,
        mouth: Option<f32>,
        yaw: Option<f32>,
    ) -> PyResult<FrameResult> {
        let signals = match (ear, mouth, yaw) {
            (Some(ear), Some(mouth), Some(yaw)) => Some(FaceSignals { ear, mouth, yaw }),
            (None, None, None) => None,
            _ => {
                return Err(PyValueError::new_err(
                    "pass all of ear, mouth and yaw, or none",
                ));
            }
        };
        let (status, actions) = self.inner.observe(timestamp_ms, face_count, signals);
        Ok(FrameResult::new(status, actions, face_count, signals))
    }
}

/// Ed25519 key for issuing tickets and signing receipts. Keep it on your server.
#[pyclass(module = "faceliveness")]
struct SigningKey {
    inner: attest::SigningKey,
}

#[pymethods]
impl SigningKey {
    /// A new random key.
    #[staticmethod]
    fn generate() -> PyResult<Self> {
        Ok(Self {
            inner: attest::SigningKey::generate().map_err(to_py)?,
        })
    }

    /// Loads a key saved with `to_bytes()` (32 bytes).
    #[staticmethod]
    fn from_bytes(secret: &[u8]) -> PyResult<Self> {
        Ok(Self {
            inner: attest::SigningKey::from_bytes(&key32(secret, "signing key")?),
        })
    }

    /// The 32-byte secret: store it like a password.
    fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes().to_vec()
    }

    /// The 32-byte public key, for verifying tickets and receipts.
    #[getter]
    fn public_key(&self) -> Vec<u8> {
        self.inner.public_key().to_bytes().to_vec()
    }

    /// A signed ticket (JWT) for `challenges`, or for `count` random ones if not given.
    #[pyo3(signature = (challenges=None, *, count=3, ttl_secs=120, now=None))]
    fn issue_ticket(
        &self,
        challenges: Option<Vec<String>>,
        count: usize,
        ttl_secs: u64,
        now: Option<u64>,
    ) -> PyResult<String> {
        let now = now_or_clock(now);
        let issued = match challenges {
            Some(names) => self
                .inner
                .issue_ticket(parse_challenges(names)?, now, ttl_secs),
            None => self.inner.issue_random_ticket(count, now, ttl_secs),
        };
        Ok(issued.map_err(to_py)?.1)
    }

    fn __repr__(&self) -> &'static str {
        "SigningKey(<secret>)"
    }
}

/// A verified challenge ticket.
#[pyclass(frozen, get_all, module = "faceliveness")]
struct Ticket {
    nonce: String,
    challenges: Vec<&'static str>,
    issued_at: u64,
    expires_at: u64,
}

/// A verified liveness receipt.
#[pyclass(frozen, get_all, module = "faceliveness")]
struct Receipt {
    /// The ticket's nonce: accept each nonce only once.
    nonce: String,
    challenges: Vec<&'static str>,
    passed: bool,
    fail_reason: Option<&'static str>,
    frames: u32,
    frames_sha256: String,
    real_score: Option<f32>,
    issued_at: u64,
    expires_at: u64,
}

#[pymethods]
impl Receipt {
    fn __repr__(&self) -> String {
        format!(
            "Receipt(passed={}, fail_reason={:?}, nonce={:?}, frames={})",
            self.passed, self.fail_reason, self.nonce, self.frames
        )
    }
}

fn names(challenges: &[Challenge]) -> Vec<&'static str> {
    challenges.iter().map(|c| c.as_str()).collect()
}

/// Checks a ticket's signature and expiry; raises ValueError if invalid.
#[pyfunction]
#[pyo3(signature = (token, public_key, *, now=None))]
fn verify_ticket(token: &str, public_key: &[u8], now: Option<u64>) -> PyResult<Ticket> {
    let t = self::public_key(public_key)?
        .verify_ticket(token, now_or_clock(now))
        .map_err(to_py)?;
    Ok(Ticket {
        nonce: t.nonce,
        challenges: names(&t.challenges),
        issued_at: t.issued_at,
        expires_at: t.expires_at,
    })
}

/// Checks a receipt's signature and expiry; raises ValueError if invalid.
#[pyfunction]
#[pyo3(signature = (token, public_key, *, now=None))]
fn verify_receipt(token: &str, public_key: &[u8], now: Option<u64>) -> PyResult<Receipt> {
    let r = self::public_key(public_key)?
        .verify_receipt(token, now_or_clock(now))
        .map_err(to_py)?;
    Ok(Receipt {
        passed: r.passed(),
        nonce: r.nonce,
        challenges: names(&r.challenges),
        fail_reason: r.fail_reason.map(|f| f.as_str()),
        frames: r.frames,
        frames_sha256: r.frames_sha256,
        real_score: r.real_score,
        issued_at: r.issued_at,
        expires_at: r.expires_at,
    })
}

#[pyfunction]
fn version() -> &'static str {
    liveness_core::version()
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<Engine>()?;
    m.add_class::<Session>()?;
    m.add_class::<FrameResult>()?;
    m.add_class::<Signals>()?;
    m.add_class::<FaceBox>()?;
    m.add_class::<SigningKey>()?;
    m.add_class::<Ticket>()?;
    m.add_class::<Receipt>()?;
    m.add_function(wrap_pyfunction!(verify_ticket, m)?)?;
    m.add_function(wrap_pyfunction!(verify_receipt, m)?)?;
    Ok(())
}
