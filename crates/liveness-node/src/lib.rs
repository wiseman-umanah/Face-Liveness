#![deny(clippy::all)]

use std::sync::{Arc, Mutex};

use liveness_core::challenge::{Challenge, Status};
use liveness_core::error::LivenessError;
use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::session::{LivenessConfig, LivenessSession};
use liveness_core::signals::FaceSignals;
use liveness_core::vision::{FrameResult as CoreFrameResult, LivenessEngine};
use napi::bindgen_prelude::*;
use napi_derive::napi;

fn to_js(e: LivenessError) -> Error {
  match e {
    LivenessError::Model(_) => Error::new(napi::Status::GenericFailure, e.to_string()),
    _ => Error::new(napi::Status::InvalidArg, e.to_string()),
  }
}

fn invalid(msg: impl Into<String>) -> Error {
  Error::new(napi::Status::InvalidArg, msg.into())
}

fn non_negative(name: &str, v: i64) -> Result<u64> {
  u64::try_from(v).map_err(|_| invalid(format!("{name} must not be negative")))
}

#[napi(object)]
#[derive(Default)]
pub struct SessionOptions {
  /// Fail if the whole check takes longer than this (default 30000).
  pub total_timeout_ms: Option<i64>,
  /// Fail if one challenge takes longer than this (default 10000).
  pub step_timeout_ms: Option<i64>,
  /// Fail if the face is out of view longer than this (default 1000).
  pub face_lost_ms: Option<i64>,
  /// Only for `random`: fixes the challenge order. Leave unset in production so the
  /// order comes from the OS and can't be predicted (and pre-recorded).
  pub seed: Option<BigInt>,
}

impl SessionOptions {
  fn config(&self) -> Result<LivenessConfig> {
    let mut c = LivenessConfig::default();
    if let Some(v) = self.total_timeout_ms {
      c.session.total_timeout_ms = non_negative("totalTimeoutMs", v)?;
    }
    if let Some(v) = self.step_timeout_ms {
      c.session.step_timeout_ms = non_negative("stepTimeoutMs", v)?;
    }
    if let Some(v) = self.face_lost_ms {
      c.session.face_lost_ms = non_negative("faceLostMs", v)?;
    }
    Ok(c)
  }

  fn seed(&self) -> Result<u64> {
    match &self.seed {
      Some(seed) => Ok(seed.get_u64().1),
      None => getrandom::u64().map_err(|e| {
        Error::new(
          napi::Status::GenericFailure,
          format!("no OS randomness: {e}"),
        )
      }),
    }
  }
}

#[napi(object)]
pub struct FrameOptions {
  pub width: u32,
  pub height: u32,
  /// Capture time in milliseconds; drives every timeout.
  pub timestamp_ms: i64,
  /// Default 'rgb' (3 bytes per pixel).
  #[napi(ts_type = "PixelFormat")]
  pub format: Option<String>,
  /// Bytes per row; defaults to width x bytes per pixel.
  pub stride: Option<u32>,
  /// Clockwise degrees (0, 90, 180, 270) that turn the frame upright.
  pub rotation: Option<u32>,
  /// True for selfie-style mirrored frames.
  pub mirrored: Option<bool>,
}

/// Owned copy of a frame, so it can be processed on a worker thread.
struct OwnedFrame {
  data: Vec<u8>,
  width: u32,
  height: u32,
  stride: u32,
  format: PixelFormat,
  rotation: Rotation,
  mirrored: bool,
  timestamp_ms: u64,
}

impl OwnedFrame {
  fn new(data: &[u8], o: &FrameOptions) -> Result<Self> {
    let format: PixelFormat = o
      .format
      .as_deref()
      .unwrap_or("rgb")
      .parse()
      .map_err(to_js)?;
    let bytes_per_pixel = match format {
      PixelFormat::Rgb8 | PixelFormat::Bgr8 => 3,
      PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
      PixelFormat::Nv21 => 1,
    };
    Ok(Self {
      data: data.to_vec(),
      width: o.width,
      height: o.height,
      stride: o.stride.unwrap_or(o.width * bytes_per_pixel),
      format,
      rotation: Rotation::from_degrees(o.rotation.unwrap_or(0)).map_err(to_js)?,
      mirrored: o.mirrored.unwrap_or(false),
      timestamp_ms: non_negative("timestampMs", o.timestamp_ms)?,
    })
  }

  fn frame(&self) -> Frame<'_> {
    Frame {
      data: &self.data,
      width: self.width,
      height: self.height,
      stride: self.stride,
      format: self.format,
      rotation: self.rotation,
      mirrored: self.mirrored,
      timestamp_ms: self.timestamp_ms,
    }
  }
}

#[napi(object)]
pub struct Signals {
  /// Eye aspect ratio: ~0.25-0.35 open, drops sharply during a blink.
  pub ear: f64,
  /// Inner-lip height / width: ~0.05 closed, >0.4 wide open.
  pub mouth: f64,
  /// Nose position between the face edges: ~0.5 facing the camera.
  pub yaw: f64,
}

impl From<FaceSignals> for Signals {
  fn from(s: FaceSignals) -> Self {
    Self {
      ear: s.ear.into(),
      mouth: s.mouth.into(),
      yaw: s.yaw.into(),
    }
  }
}

/// A detected face in upright pixel coordinates.
#[napi(object)]
pub struct FaceBox {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
  pub score: f64,
}

#[napi(object)]
pub struct FrameResult {
  #[napi(ts_type = "LivenessStatus")]
  pub status: String,
  /// True once passed or failed; later frames won't change the outcome.
  pub done: bool,
  /// What the user should do now, while in progress.
  #[napi(ts_type = "Challenge")]
  pub current_challenge: Option<String>,
  /// Challenges left including the current one, while in progress.
  pub remaining: u32,
  #[napi(ts_type = "FailReason")]
  pub fail_reason: Option<String>,
  /// Actions recognised in this frame.
  #[napi(ts_type = "Challenge[]")]
  pub actions: Vec<String>,
  pub face_count: u32,
  pub signals: Option<Signals>,
  /// Empty for `Session.update`.
  pub faces: Vec<FaceBox>,
  /// 478 face-mesh landmarks [x, y] of the primary face, in upright pixels.
  #[napi(ts_type = "Array<[number, number]>")]
  pub landmarks: Option<Vec<Vec<f64>>>,
}

impl FrameResult {
  fn new(
    status: Status,
    actions: &[Challenge],
    face_count: usize,
    signals: Option<FaceSignals>,
  ) -> Self {
    let (name, current, remaining, reason) = match status {
      Status::InProgress { current, remaining } => {
        ("in_progress", Some(current.as_str()), remaining, None)
      }
      Status::Passed => ("passed", None, 0, None),
      Status::Failed(r) => ("failed", None, 0, Some(r.as_str())),
    };
    Self {
      status: name.to_string(),
      done: name != "in_progress",
      current_challenge: current.map(str::to_string),
      remaining: remaining as u32,
      fail_reason: reason.map(str::to_string),
      actions: actions.iter().map(|a| a.as_str().to_string()).collect(),
      face_count: face_count as u32,
      signals: signals.map(Signals::from),
      faces: vec![],
      landmarks: None,
    }
  }

  fn from_engine(r: CoreFrameResult) -> Self {
    let mut out = Self::new(
      r.status,
      &r.actions,
      r.analysis.face_count(),
      r.analysis.signals,
    );
    out.faces = r
      .analysis
      .faces
      .iter()
      .map(|f| FaceBox {
        x: f.x.into(),
        y: f.y.into(),
        width: f.width.into(),
        height: f.height.into(),
        score: f.score.into(),
      })
      .collect();
    out.landmarks = r
      .analysis
      .landmarks
      .map(|l| l.iter().map(|p| vec![p.x.into(), p.y.into()]).collect());
    out
  }
}

fn parse_challenges(names: &[String]) -> Result<Vec<Challenge>> {
  names.iter().map(|n| n.parse().map_err(to_js)).collect()
}

fn challenge_names(c: &[Challenge]) -> Vec<String> {
  c.iter().map(|c| c.as_str().to_string()).collect()
}

fn lock(engine: &Mutex<LivenessEngine>) -> Result<std::sync::MutexGuard<'_, LivenessEngine>> {
  engine.lock().map_err(|_| {
    Error::new(
      napi::Status::GenericFailure,
      "engine poisoned by an earlier panic",
    )
  })
}

/// Frames in, liveness verdict out: face detection, landmarks and the challenge flow.
/// One engine is one liveness attempt; create a new one per attempt.
#[napi]
pub struct Engine {
  inner: Arc<Mutex<LivenessEngine>>,
  challenges: Vec<String>,
}

#[napi]
impl Engine {
  #[napi(constructor)]
  pub fn new(
    #[napi(ts_arg_type = "Challenge[]")] challenges: Vec<String>,
    options: Option<SessionOptions>,
  ) -> Result<Self> {
    let config = options.unwrap_or_default().config()?;
    let engine = LivenessEngine::new(parse_challenges(&challenges)?, config).map_err(to_js)?;
    Ok(Self::wrap(engine))
  }

  /// `count` challenges in random order.
  #[napi(factory)]
  pub fn random(count: u32, options: Option<SessionOptions>) -> Result<Self> {
    let options = options.unwrap_or_default();
    let engine =
      LivenessEngine::random(count as usize, options.seed()?, options.config()?).map_err(to_js)?;
    Ok(Self::wrap(engine))
  }

  fn wrap(engine: LivenessEngine) -> Self {
    let challenges = challenge_names(engine.challenges());
    Self {
      inner: Arc::new(Mutex::new(engine)),
      challenges,
    }
  }

  #[napi(getter, ts_return_type = "Challenge[]")]
  pub fn challenges(&self) -> Vec<String> {
    self.challenges.clone()
  }

  /// Processes one frame on the calling thread (blocks for the inference time).
  #[napi]
  pub fn process(&self, frame: Uint8Array, options: FrameOptions) -> Result<FrameResult> {
    let owned = OwnedFrame::new(&frame, &options)?;
    let result = lock(&self.inner)?.process(&owned.frame()).map_err(to_js)?;
    Ok(FrameResult::from_engine(result))
  }

  /// Processes one frame on Node's worker pool, keeping the event loop free.
  /// Await each call before sending the next frame.
  #[napi(ts_return_type = "Promise<FrameResult>")]
  pub fn process_async(
    &self,
    frame: Uint8Array,
    options: FrameOptions,
  ) -> Result<AsyncTask<ProcessTask>> {
    let frame = OwnedFrame::new(&frame, &options)?;
    Ok(AsyncTask::new(ProcessTask {
      engine: self.inner.clone(),
      frame,
    }))
  }
}

pub struct ProcessTask {
  engine: Arc<Mutex<LivenessEngine>>,
  frame: OwnedFrame,
}

impl Task for ProcessTask {
  type Output = CoreFrameResult;
  type JsValue = FrameResult;

  fn compute(&mut self) -> Result<Self::Output> {
    lock(&self.engine)?
      .process(&self.frame.frame())
      .map_err(to_js)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(FrameResult::from_engine(output))
  }
}

#[napi(object)]
#[derive(Default)]
pub struct Observation {
  /// Faces in view (default 1).
  pub face_count: Option<u32>,
  pub ear: Option<f64>,
  pub mouth: Option<f64>,
  pub yaw: Option<f64>,
}

/// Liveness decisions from measurements you compute yourself (no image processing).
#[napi]
pub struct Session {
  inner: LivenessSession,
}

#[napi]
impl Session {
  #[napi(constructor)]
  pub fn new(
    #[napi(ts_arg_type = "Challenge[]")] challenges: Vec<String>,
    options: Option<SessionOptions>,
  ) -> Result<Self> {
    let config = options.unwrap_or_default().config()?;
    let inner = LivenessSession::new(parse_challenges(&challenges)?, config).map_err(to_js)?;
    Ok(Self { inner })
  }

  #[napi(factory)]
  pub fn random(count: u32, options: Option<SessionOptions>) -> Result<Self> {
    let options = options.unwrap_or_default();
    let inner =
      LivenessSession::random(count as usize, options.seed()?, options.config()?).map_err(to_js)?;
    Ok(Self { inner })
  }

  #[napi(getter, ts_return_type = "Challenge[]")]
  pub fn challenges(&self) -> Vec<String> {
    challenge_names(self.inner.challenges())
  }

  /// One frame: how many faces are visible, and the main face's measurements if known.
  #[napi]
  pub fn update(
    &mut self,
    timestamp_ms: i64,
    observation: Option<Observation>,
  ) -> Result<FrameResult> {
    let o = observation.unwrap_or_default();
    let signals = match (o.ear, o.mouth, o.yaw) {
      (Some(ear), Some(mouth), Some(yaw)) => Some(FaceSignals {
        ear: ear as f32,
        mouth: mouth as f32,
        yaw: yaw as f32,
      }),
      (None, None, None) => None,
      _ => return Err(invalid("pass all of ear, mouth and yaw, or none")),
    };
    let face_count = o.face_count.unwrap_or(1) as usize;
    let (status, actions) = self.inner.observe(
      non_negative("timestampMs", timestamp_ms)?,
      face_count,
      signals,
    );
    Ok(FrameResult::new(status, &actions, face_count, signals))
  }
}

#[napi]
pub fn version() -> String {
  liveness_core::version().to_string()
}
