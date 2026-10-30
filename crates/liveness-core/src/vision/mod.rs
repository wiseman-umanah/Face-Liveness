//! Raw frames in, face measurements out: BlazeFace detection + face-mesh landmarks,
//! run with bundled ONNX models on pure-Rust inference (tract), so it builds anywhere
//! Rust does, including WASM and mobile.

mod antispoof;
mod detector;
mod engine;
mod image;
mod landmarks;

pub use engine::{FrameResult, LivenessEngine};

use tract_onnx::prelude::*;

use self::image::{Border, Roi, UprightView};
use crate::actions::Point;
use crate::error::{LivenessError, Result};
use crate::frame::Frame;
use crate::signals::FaceSignals;

/// MediaPipe BlazeFace short-range, converted to ONNX (see models/README.md).
pub const DETECTOR_MODEL: &[u8] = include_bytes!("../../models/face_detector.onnx");
/// MediaPipe face-mesh landmark model (478 points), converted to ONNX.
pub const LANDMARK_MODEL: &[u8] = include_bytes!("../../models/face_landmarks_detector.onnx");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyzerConfig {
    /// Minimum detector confidence for a face to count.
    pub min_detection_score: f32,
    /// Minimum landmark-model confidence that the crop still contains a face.
    pub min_presence: f32,
    /// Run the passive anti-spoofing models on the primary face.
    pub antispoof: bool,
    /// Score every Nth frame that has a face (1 = every frame). The score is used as a
    /// session mean, so sampling barely changes it; each run costs about a third of the
    /// landmark pipeline.
    pub antispoof_every: u32,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        // MediaPipe's defaults.
        Self {
            min_detection_score: 0.5,
            min_presence: 0.5,
            antispoof: true,
            antispoof_every: 3,
        }
    }
}

/// A detected face box in upright pixel coordinates (top-left origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub score: f32,
}

/// Everything found in one frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceAnalysis {
    /// Every face the detector found, most confident first.
    pub faces: Vec<FaceBox>,
    /// 478 face-mesh landmarks of the primary face, in upright pixel coordinates.
    pub landmarks: Option<Vec<Point>>,
    /// Measurements of the primary face, for the action detectors.
    pub signals: Option<FaceSignals>,
    /// Passive anti-spoofing: probability (0-1) that the primary face is a real face rather
    /// than a photo or screen, on frames where it was measured (see
    /// [`AnalyzerConfig::antispoof_every`]). Uncalibrated for your cameras; see `models/README.md`.
    pub real_score: Option<f32>,
}

impl FaceAnalysis {
    /// Faces in view: detector count, or 1 if only tracking still sees the face.
    pub fn face_count(&self) -> usize {
        self.faces.len().max(self.landmarks.is_some() as usize)
    }
}

type Model = Arc<TypedRunnableModel>;

/// NHWC input for a square RGB model of side `size`.
fn nhwc(size: usize) -> [usize; 4] {
    [1, size, size, 3]
}

fn load(bytes: &[u8], shape: [usize; 4]) -> Result<Model> {
    let model_err = |e: TractError| LivenessError::Model(e.to_string());
    tract_onnx::onnx()
        .model_for_read(&mut &*bytes)
        .and_then(|m| m.with_input_fact(0, f32::fact(shape).into()))
        .and_then(|m| m.into_optimized())
        .and_then(|m| m.into_runnable())
        .map_err(model_err)
}

fn run(model: &Model, input: Vec<f32>, shape: [usize; 4]) -> Result<TVec<TValue>> {
    let tensor =
        Tensor::from_shape(&shape, &input).map_err(|e| LivenessError::Model(e.to_string()))?;
    model
        .run(tvec!(tensor.into()))
        .map_err(|e| LivenessError::Model(e.to_string()))
}

fn floats(value: &TValue) -> Result<&[f32]> {
    value
        .try_as_plain_ram()
        .and_then(|v| v.as_slice::<f32>())
        .map_err(|e| LivenessError::Model(e.to_string()))
}

/// Finds faces and measures the primary one, frame after frame.
///
/// Keeps state between frames: once a face is found, the next crop is taken from the
/// previous landmarks (as MediaPipe does), which is steadier than re-cropping from the
/// detector. Call [`reset`](Self::reset) when switching to an unrelated video.
pub struct FaceAnalyzer {
    detector: Model,
    landmarker: Model,
    /// MiniFASNet models paired with their crop scale; empty if anti-spoofing is off.
    antispoof: Vec<(Model, f32)>,
    anchors: Vec<(f32, f32)>,
    config: AnalyzerConfig,
    tracked: Option<Roi>,
    /// Frames with a face since the last reset, for `antispoof_every`.
    faces_seen: u32,
}

impl FaceAnalyzer {
    /// Uses the bundled models.
    pub fn new(config: AnalyzerConfig) -> Result<Self> {
        Self::from_models(DETECTOR_MODEL, LANDMARK_MODEL, config)
    }

    /// Uses models supplied as ONNX bytes (same inputs/outputs as the bundled ones).
    pub fn from_models(detector: &[u8], landmarks: &[u8], config: AnalyzerConfig) -> Result<Self> {
        Ok(Self {
            detector: load(detector, nhwc(detector::INPUT_SIZE))?,
            landmarker: load(landmarks, nhwc(landmarks::INPUT_SIZE))?,
            antispoof: if config.antispoof {
                let shape = [1, 3, antispoof::INPUT_SIZE, antispoof::INPUT_SIZE];
                vec![
                    (load(antispoof::MODEL_V2, shape)?, antispoof::SCALES[0]),
                    (load(antispoof::MODEL_V1SE, shape)?, antispoof::SCALES[1]),
                ]
            } else {
                vec![]
            },
            anchors: detector::anchors(),
            config,
            tracked: None,
            faces_seen: 0,
        })
    }

    pub fn reset(&mut self) {
        self.tracked = None;
        self.faces_seen = 0;
    }

    pub fn analyze(&mut self, frame: &Frame) -> Result<FaceAnalysis> {
        frame.validate()?;
        let view = UprightView::new(frame);
        let detections = self.detect(&view)?;

        let mut analysis = FaceAnalysis {
            faces: detections
                .iter()
                .map(|d| FaceBox {
                    x: d.cx - d.width / 2.0,
                    y: d.cy - d.height / 2.0,
                    width: d.width,
                    height: d.height,
                    score: d.score,
                })
                .collect(),
            ..FaceAnalysis::default()
        };

        // Prefer the tracked crop; fall back to the best detection if tracking lost the face.
        let candidates = self
            .tracked
            .into_iter()
            .chain(detections.first().map(landmarks::roi_from_detection));
        self.tracked = None;
        for roi in candidates {
            if let Some(points) = self.landmarks(&view, &roi)? {
                self.tracked = Some(landmarks::roi_from_landmarks(&points));
                analysis.signals = Some(landmarks::signals(&points));
                if self
                    .faces_seen
                    .is_multiple_of(self.config.antispoof_every.max(1))
                {
                    analysis.real_score = self.real_score(&view, &points)?;
                }
                self.faces_seen = self.faces_seen.wrapping_add(1);
                analysis.landmarks = Some(points);
                break;
            }
        }
        Ok(analysis)
    }

    fn detect(&self, view: &UprightView) -> Result<Vec<detector::Detection>> {
        // Letterbox: the whole frame, centered in a square, padded with black.
        let side = view.width.max(view.height);
        let input = Roi {
            cx: view.width / 2.0,
            cy: view.height / 2.0,
            width: side,
            height: side,
            angle: 0.0,
        };
        let size = detector::INPUT_SIZE;
        let out = run(
            &self.detector,
            view.crop(&input, size, (-1.0, 1.0), Border::Zero),
            nhwc(size),
        )?;
        let (regressors, scores) = match out.as_slice() {
            [a, b] if a.len() > b.len() => (floats(a)?, floats(b)?),
            [a, b] => (floats(b)?, floats(a)?),
            _ => return Err(LivenessError::Model("detector must have 2 outputs".into())),
        };
        Ok(detector::decode(
            regressors,
            scores,
            &self.anchors,
            &input,
            self.config.min_detection_score,
        ))
    }

    /// Mean "real face" probability over the anti-spoofing models, or None if they are off.
    fn real_score(&self, view: &UprightView, points: &[Point]) -> Result<Option<f32>> {
        if self.antispoof.is_empty() {
            return Ok(None);
        }
        let face = antispoof::face_box(points);
        let size = antispoof::INPUT_SIZE;
        let mut total = 0.0;
        for (model, scale) in &self.antispoof {
            let bounds = antispoof::scaled_box(view.width, view.height, face, *scale);
            let out = run(model, antispoof::input(view, bounds), [1, 3, size, size])?;
            total += antispoof::real_probability(floats(&out[0])?);
        }
        Ok(Some(total / self.antispoof.len() as f32))
    }

    /// Landmarks inside `roi`, or None if the model says there is no face there.
    fn landmarks(&self, view: &UprightView, roi: &Roi) -> Result<Option<Vec<Point>>> {
        let size = landmarks::INPUT_SIZE;
        let out = run(
            &self.landmarker,
            view.crop(roi, size, (0.0, 1.0), Border::Replicate),
            nhwc(size),
        )?;
        // Outputs: landmarks [1,1,1,1434], face-presence logit [1,1,1,1], and an unused [1,1].
        let mut points = None;
        let mut presence = None;
        for value in &out {
            match value.shape() {
                [1, 1, 1, n] if *n == landmarks::NUM_LANDMARKS * 3 => points = Some(floats(value)?),
                [1, 1, 1, 1] => presence = Some(floats(value)?[0]),
                _ => {}
            }
        }
        let (Some(points), Some(presence)) = (points, presence) else {
            return Err(LivenessError::Model(
                "unexpected landmark model outputs".into(),
            ));
        };
        if detector::sigmoid(presence) < self.config.min_presence {
            return Ok(None);
        }
        Ok(Some(landmarks::project(points, roi)))
    }
}
