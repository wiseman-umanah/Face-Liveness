//! Browser (WebAssembly) bindings: the same engine as the Python and Node packages, running
//! on the user's device. Results here are for live feedback only; a server must re-check
//! frames (Engine.fromTicket in Python/Node) for a result it can trust.

use js_sys::{Array, Object, Reflect};
use liveness_core::attest::PublicKey;
use liveness_core::challenge::{Challenge, Status};
use liveness_core::error::LivenessError;
use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::session::{LivenessConfig, LivenessSession};
use liveness_core::signals::FaceSignals;
use liveness_core::vision::{FrameResult as CoreFrameResult, LivenessEngine};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
export type Challenge = 'blink' | 'turn_left' | 'turn_right' | 'open_mouth'
export type PixelFormat = 'rgb' | 'bgr' | 'rgba' | 'bgra' | 'nv21'
export type LivenessStatus = 'in_progress' | 'passed' | 'failed'
export type FailReason = 'timeout' | 'step_timeout' | 'face_lost' | 'multiple_faces' | 'spoof_suspected'

export interface SessionOptions {
  /** Fail if the whole check takes longer than this (default 30000). */
  totalTimeoutMs?: number
  /** Fail if one challenge takes longer than this (default 10000). */
  stepTimeoutMs?: number
  /** Fail if the face is out of view longer than this (default 1000). */
  faceLostMs?: number
  /** Fail a completed session whose mean realScore is below this (off by default). */
  minRealScore?: number
}

export interface FrameOptions {
  width: number
  height: number
  /** Capture time in milliseconds; drives every timeout. */
  timestampMs: number
  /** Default 'rgba', the layout of canvas ImageData. */
  format?: PixelFormat
  /** Bytes per row; defaults to width x bytes per pixel. */
  stride?: number
  /** Clockwise degrees (0, 90, 180, 270) that turn the frame upright. */
  rotation?: number
  /** True for selfie-style mirrored frames. */
  mirrored?: boolean
}

export interface Signals { ear: number; mouth: number; yaw: number }
export interface FaceBox { x: number; y: number; width: number; height: number; score: number }

export interface FrameResult {
  status: LivenessStatus
  /** True once passed or failed; later frames won't change the outcome. */
  done: boolean
  currentChallenge?: Challenge
  remaining: number
  failReason?: FailReason
  /** Actions recognised in this frame. */
  actions: Challenge[]
  faceCount: number
  signals?: Signals
  faces: FaceBox[]
  /** 478 face-mesh landmarks [x, y] of the primary face, in upright pixels. */
  landmarks?: Array<[number, number]>
  /** Passive anti-spoofing score (0-1), on every 3rd frame with a face. Uncalibrated. */
  realScore?: number
}

export interface Ticket { nonce: string; challenges: Challenge[]; issuedAt: number; expiresAt: number }
export interface Receipt {
  nonce: string
  challenges: Challenge[]
  passed: boolean
  failReason?: FailReason
  frames: number
  framesSha256: string
  realScore?: number
  issuedAt: number
  expiresAt: number
}
"#;

fn js_err(e: LivenessError) -> JsError {
    JsError::new(&e.to_string())
}

fn get(obj: &JsValue, key: &str) -> Option<JsValue> {
    if obj.is_undefined() || obj.is_null() {
        return None;
    }
    Reflect::get(obj, &key.into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

fn get_u64(obj: &JsValue, key: &str) -> Result<Option<u64>, JsError> {
    match get(obj, key).map(|v| v.as_f64()) {
        None => Ok(None),
        Some(Some(n)) if n >= 0.0 && n.fract() == 0.0 => Ok(Some(n as u64)),
        Some(_) => Err(JsError::new(&format!(
            "{key} must be a non-negative integer"
        ))),
    }
}

fn set(obj: &Object, key: &str, value: impl Into<JsValue>) {
    Reflect::set(obj, &key.into(), &value.into()).expect("setting a property on a plain object");
}

fn config(options: &JsValue) -> Result<LivenessConfig, JsError> {
    let mut c = LivenessConfig::default();
    if let Some(v) = get_u64(options, "totalTimeoutMs")? {
        c.session.total_timeout_ms = v;
    }
    if let Some(v) = get_u64(options, "stepTimeoutMs")? {
        c.session.step_timeout_ms = v;
    }
    if let Some(v) = get_u64(options, "faceLostMs")? {
        c.session.face_lost_ms = v;
    }
    c.min_real_score = get(options, "minRealScore")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    Ok(c)
}

fn parse_challenges(names: Vec<String>) -> Result<Vec<Challenge>, JsError> {
    names.iter().map(|n| n.parse().map_err(js_err)).collect()
}

fn names(challenges: &[Challenge]) -> Array {
    challenges
        .iter()
        .map(|c| JsValue::from_str(c.as_str()))
        .collect()
}

fn random_seed() -> Result<u64, JsError> {
    let mut bytes = [0u8; 8];
    getrandom_fill(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(target_arch = "wasm32")]
fn getrandom_fill(bytes: &mut [u8]) -> Result<(), JsError> {
    getrandom::fill(bytes).map_err(|e| JsError::new(&format!("no browser randomness: {e}")))
}

#[cfg(not(target_arch = "wasm32"))]
fn getrandom_fill(_bytes: &mut [u8]) -> Result<(), JsError> {
    Err(JsError::new("liveness-wasm is meant for wasm32 targets"))
}

fn status_object(
    status: &Status,
    actions: &[Challenge],
    face_count: usize,
    signals: Option<FaceSignals>,
) -> Object {
    let out = Object::new();
    let (name, current, remaining, reason) = match status {
        Status::InProgress { current, remaining } => {
            ("in_progress", Some(current.as_str()), *remaining, None)
        }
        Status::Passed => ("passed", None, 0, None),
        Status::Failed(r) => ("failed", None, 0, Some(r.as_str())),
    };
    set(&out, "status", name);
    set(&out, "done", name != "in_progress");
    if let Some(c) = current {
        set(&out, "currentChallenge", c);
    }
    set(&out, "remaining", remaining as u32);
    if let Some(r) = reason {
        set(&out, "failReason", r);
    }
    set(&out, "actions", names(actions));
    set(&out, "faceCount", face_count as u32);
    if let Some(s) = signals {
        let o = Object::new();
        set(&o, "ear", s.ear);
        set(&o, "mouth", s.mouth);
        set(&o, "yaw", s.yaw);
        set(&out, "signals", o);
    }
    set(&out, "faces", Array::new());
    out
}

fn engine_result(r: CoreFrameResult) -> JsValue {
    let out = status_object(
        &r.status,
        &r.actions,
        r.analysis.face_count(),
        r.analysis.signals,
    );
    let faces: Array = r
        .analysis
        .faces
        .iter()
        .map(|f| {
            let o = Object::new();
            set(&o, "x", f.x);
            set(&o, "y", f.y);
            set(&o, "width", f.width);
            set(&o, "height", f.height);
            set(&o, "score", f.score);
            JsValue::from(o)
        })
        .collect();
    set(&out, "faces", faces);
    if let Some(points) = r.analysis.landmarks {
        let list: Array = points
            .iter()
            .map(|p| JsValue::from(Array::of2(&p.x.into(), &p.y.into())))
            .collect();
        set(&out, "landmarks", list);
    }
    if let Some(score) = r.analysis.real_score {
        set(&out, "realScore", score);
    }
    out.into()
}

/// Frames in, liveness verdict out. One engine is one attempt; create a new one per attempt.
/// Model loading takes a moment, so create it before showing the camera.
#[wasm_bindgen]
pub struct Engine {
    inner: LivenessEngine,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new(
        #[wasm_bindgen(unchecked_param_type = "Challenge[]")] challenges: Vec<String>,
        #[wasm_bindgen(unchecked_param_type = "SessionOptions | undefined")] options: JsValue,
    ) -> Result<Engine, JsError> {
        let inner = LivenessEngine::new(parse_challenges(challenges)?, config(&options)?)
            .map_err(js_err)?;
        Ok(Self { inner })
    }

    /// `count` challenges in an order drawn from the browser's crypto randomness.
    pub fn random(
        count: u32,
        #[wasm_bindgen(unchecked_param_type = "SessionOptions | undefined")] options: JsValue,
    ) -> Result<Engine, JsError> {
        let inner = LivenessEngine::random(count as usize, random_seed()?, config(&options)?)
            .map_err(js_err)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(getter, unchecked_return_type = "Challenge[]")]
    pub fn challenges(&self) -> Array {
        names(self.inner.challenges())
    }

    /// Processes one frame, e.g. `ctx.getImageData(...).data` with format 'rgba' (the default).
    #[wasm_bindgen(unchecked_return_type = "FrameResult")]
    pub fn process(
        &mut self,
        pixels: &[u8],
        #[wasm_bindgen(unchecked_param_type = "FrameOptions")] options: JsValue,
    ) -> Result<JsValue, JsError> {
        let width =
            get_u64(&options, "width")?.ok_or_else(|| JsError::new("width is required"))? as u32;
        let height =
            get_u64(&options, "height")?.ok_or_else(|| JsError::new("height is required"))? as u32;
        let timestamp_ms = get_u64(&options, "timestampMs")?
            .ok_or_else(|| JsError::new("timestampMs is required"))?;
        let format: PixelFormat = get(&options, "format")
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "rgba".into())
            .parse()
            .map_err(js_err)?;
        let bytes_per_pixel = match format {
            PixelFormat::Rgb8 | PixelFormat::Bgr8 => 3,
            PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
            PixelFormat::Nv21 => 1,
        };
        let rotation = get_u64(&options, "rotation")?.unwrap_or(0);
        let frame = Frame {
            data: pixels,
            width,
            height,
            stride: get_u64(&options, "stride")?.map_or(width * bytes_per_pixel, |s| s as u32),
            format,
            rotation: Rotation::from_degrees(u32::try_from(rotation).unwrap_or(u32::MAX))
                .map_err(js_err)?,
            mirrored: get(&options, "mirrored")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            timestamp_ms,
        };
        Ok(engine_result(self.inner.process(&frame).map_err(js_err)?))
    }
}

/// The challenge flow from measurements you compute yourself (no image processing).
#[wasm_bindgen]
pub struct Session {
    inner: LivenessSession,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new(
        #[wasm_bindgen(unchecked_param_type = "Challenge[]")] challenges: Vec<String>,
        #[wasm_bindgen(unchecked_param_type = "SessionOptions | undefined")] options: JsValue,
    ) -> Result<Session, JsError> {
        let inner = LivenessSession::new(parse_challenges(challenges)?, config(&options)?)
            .map_err(js_err)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(getter, unchecked_return_type = "Challenge[]")]
    pub fn challenges(&self) -> Array {
        names(self.inner.challenges())
    }

    /// One frame: faces in view and the main face's measurements, if known.
    #[wasm_bindgen(unchecked_return_type = "FrameResult")]
    pub fn update(
        &mut self,
        timestamp_ms: f64,
        #[wasm_bindgen(
            unchecked_param_type = "{ faceCount?: number; ear?: number; mouth?: number; yaw?: number } | undefined"
        )]
        observation: JsValue,
    ) -> Result<JsValue, JsError> {
        let num = |k| get(&observation, k).and_then(|v| v.as_f64());
        let signals = match (num("ear"), num("mouth"), num("yaw")) {
            (Some(ear), Some(mouth), Some(yaw)) => Some(FaceSignals {
                ear: ear as f32,
                mouth: mouth as f32,
                yaw: yaw as f32,
            }),
            (None, None, None) => None,
            _ => return Err(JsError::new("pass all of ear, mouth and yaw, or none")),
        };
        if timestamp_ms.is_nan() || timestamp_ms < 0.0 {
            return Err(JsError::new("timestampMs must not be negative"));
        }
        let face_count = num("faceCount").map_or(1, |v| v as usize);
        let (status, actions) = self.inner.observe(timestamp_ms as u64, face_count, signals);
        Ok(status_object(&status, &actions, face_count, signals).into())
    }
}

fn public_key(bytes: &[u8]) -> Result<PublicKey, JsError> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| JsError::new(&format!("public key must be 32 bytes, got {}", bytes.len())))?;
    PublicKey::from_bytes(&bytes).map_err(js_err)
}

fn now_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// Checks a ticket from your server (signature and expiry); throws if invalid.
#[wasm_bindgen(js_name = verifyTicket, unchecked_return_type = "Ticket")]
pub fn verify_ticket(token: &str, public_key: &[u8], now: Option<f64>) -> Result<JsValue, JsError> {
    let now = now.map_or_else(now_secs, |n| n as u64);
    let t = self::public_key(public_key)?
        .verify_ticket(token, now)
        .map_err(js_err)?;
    let out = Object::new();
    set(&out, "nonce", t.nonce);
    set(&out, "challenges", names(&t.challenges));
    set(&out, "issuedAt", t.issued_at as f64);
    set(&out, "expiresAt", t.expires_at as f64);
    Ok(out.into())
}

/// Checks a receipt (signature and expiry); throws if invalid.
#[wasm_bindgen(js_name = verifyReceipt, unchecked_return_type = "Receipt")]
pub fn verify_receipt(
    token: &str,
    public_key: &[u8],
    now: Option<f64>,
) -> Result<JsValue, JsError> {
    let now = now.map_or_else(now_secs, |n| n as u64);
    let r = self::public_key(public_key)?
        .verify_receipt(token, now)
        .map_err(js_err)?;
    let out = Object::new();
    set(&out, "nonce", r.nonce.clone());
    set(&out, "challenges", names(&r.challenges));
    set(&out, "passed", r.passed());
    if let Some(f) = r.fail_reason {
        set(&out, "failReason", f.as_str());
    }
    set(&out, "frames", r.frames);
    set(&out, "framesSha256", r.frames_sha256);
    if let Some(s) = r.real_score {
        set(&out, "realScore", s);
    }
    set(&out, "issuedAt", r.issued_at as f64);
    set(&out, "expiresAt", r.expires_at as f64);
    Ok(out.into())
}

#[wasm_bindgen]
pub fn version() -> String {
    liveness_core::version().to_string()
}
