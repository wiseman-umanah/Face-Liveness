//! Runs the Rust vision pipeline on a real photo and compares it with MediaPipe's output
//! on the same image (tests/fixtures/astronaut_face_mediapipe.csv).
#![cfg(feature = "vision")]

use std::path::Path;

use image::RgbImage;
use liveness_core::actions::Point;
use liveness_core::challenge::{Challenge, FailReason, Status};
use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::session::LivenessConfig;
use liveness_core::vision::{AnalyzerConfig, FaceAnalysis, FaceAnalyzer, LivenessEngine};

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn astronaut() -> RgbImage {
    image::open(fixture_path("astronaut_face.png"))
        .unwrap()
        .to_rgb8()
}

/// MediaPipe's landmarks and (ear, mouth, yaw) for the astronaut image.
fn mediapipe_reference() -> (Vec<Point>, [f32; 3]) {
    let text = std::fs::read_to_string(fixture_path("astronaut_face_mediapipe.csv")).unwrap();
    let mut signals = [0.0; 3];
    let header = text.lines().nth(1).unwrap();
    for (i, key) in ["ear=", "mouth=", "yaw="].iter().enumerate() {
        let start = header.find(key).unwrap() + key.len();
        signals[i] = header[start..]
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
    }
    let points = text
        .lines()
        .skip(3)
        .map(|l| {
            let (x, y) = l.split_once(',').unwrap();
            Point {
                x: x.parse().unwrap(),
                y: y.parse().unwrap(),
            }
        })
        .collect();
    (points, signals)
}

fn rgb_frame(img: &RgbImage, rotation: Rotation, mirrored: bool) -> Frame<'_> {
    Frame {
        data: img.as_raw(),
        width: img.width(),
        height: img.height(),
        stride: img.width() * 3,
        format: PixelFormat::Rgb8,
        rotation,
        mirrored,
        timestamp_ms: 0,
    }
}

fn analyze(frame: &Frame) -> FaceAnalysis {
    FaceAnalyzer::new(AnalyzerConfig::default())
        .unwrap()
        .analyze(frame)
        .unwrap()
}

fn landmarks(a: &FaceAnalysis) -> &[Point] {
    a.landmarks.as_deref().expect("a face with landmarks")
}

/// Mean and max distance between two landmark sets, in pixels.
fn distance(a: &[Point], b: &[Point]) -> (f32, f32) {
    assert_eq!(a.len(), b.len());
    let d: Vec<f32> = a
        .iter()
        .zip(b)
        .map(|(p, q)| ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt())
        .collect();
    (
        d.iter().sum::<f32>() / d.len() as f32,
        d.iter().copied().fold(0.0, f32::max),
    )
}

#[test]
fn matches_mediapipe_on_a_real_face() {
    let img = astronaut();
    let analysis = analyze(&rgb_frame(&img, Rotation::Deg0, false));
    assert_eq!(analysis.faces.len(), 1);

    let (reference, [ear, mouth, yaw]) = mediapipe_reference();
    let (mean, max) = distance(landmarks(&analysis), &reference);
    eprintln!("vs MediaPipe: mean {mean:.3} px, max {max:.3} px");
    // Measured 0.34 px mean on a ~100 px face; limits are ~2x the measured values.
    assert!(
        mean < 0.75,
        "mean landmark distance {mean:.2} px (max {max:.2})"
    );

    let s = analysis.signals.unwrap();
    eprintln!("signals {s:?} vs ear={ear} mouth={mouth} yaw={yaw}");
    assert!((s.ear - ear).abs() < 0.006, "ear {} vs {ear}", s.ear);
    assert!(
        (s.mouth - mouth).abs() < 0.006,
        "mouth {} vs {mouth}",
        s.mouth
    );
    assert!((s.yaw - yaw).abs() < 0.006, "yaw {} vs {yaw}", s.yaw);
}

#[test]
fn rotated_and_mirrored_frames_give_the_same_face() {
    let img = astronaut();
    let upright = analyze(&rgb_frame(&img, Rotation::Deg0, false));

    // Buffers as a camera would deliver them, each tagged with how to turn them upright.
    let cases = [
        (image::imageops::rotate270(&img), Rotation::Deg90, false),
        (image::imageops::rotate180(&img), Rotation::Deg180, false),
        (image::imageops::rotate90(&img), Rotation::Deg270, false),
        (image::imageops::flip_horizontal(&img), Rotation::Deg0, true),
    ];
    for (buffer, rotation, mirrored) in &cases {
        let a = analyze(&rgb_frame(buffer, *rotation, *mirrored));
        let (mean, _) = distance(landmarks(&a), landmarks(&upright));
        eprintln!("{rotation:?} mirrored={mirrored}: mean {mean:.4} px");
        // Quarter turns and flips map pixels exactly, so results should be identical.
        assert!(
            mean < 0.01,
            "{rotation:?} mirrored={mirrored}: mean distance {mean:.2} px"
        );
    }
}

#[test]
fn all_pixel_formats_agree() {
    let img = astronaut();
    let (w, h) = (img.width(), img.height());
    let upright = analyze(&rgb_frame(&img, Rotation::Deg0, false));

    let rgba: Vec<u8> = img.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    let bgra: Vec<u8> = img.pixels().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    let bgr: Vec<u8> = img.pixels().flat_map(|p| [p[2], p[1], p[0]]).collect();
    let nv21 = to_nv21(&img);
    let cases = [
        // NV21 loses chroma detail, hence the looser limit (measured 0.04 px).
        (rgba.as_slice(), PixelFormat::Rgba8, w * 4, 0.01),
        (&bgra, PixelFormat::Bgra8, w * 4, 0.01),
        (&bgr, PixelFormat::Bgr8, w * 3, 0.01),
        (&nv21, PixelFormat::Nv21, w, 0.15),
    ];
    for (data, format, stride, tolerance) in cases {
        let frame = Frame {
            data,
            stride,
            format,
            ..rgb_frame(&img, Rotation::Deg0, false)
        };
        let a = analyze(&frame);
        let (mean, _) = distance(landmarks(&a), landmarks(&upright));
        eprintln!("{format:?}: mean {mean:.4} px");
        assert!(mean < tolerance, "{format:?}: mean distance {mean:.2} px");
    }
    assert_eq!((w % 2, h % 2), (0, 0));
}

/// RGB -> NV21 (BT.601 full range), averaging chroma over each 2x2 block.
fn to_nv21(img: &RgbImage) -> Vec<u8> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut out = vec![0u8; w * h * 3 / 2];
    for (x, y, p) in img.enumerate_pixels() {
        let [r, g, b] = p.0.map(f32::from);
        out[y as usize * w + x as usize] = (0.299 * r + 0.587 * g + 0.114 * b).round() as u8;
    }
    for by in (0..h).step_by(2) {
        for bx in (0..w).step_by(2) {
            let (mut u, mut v) = (0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let [r, g, b] = img
                    .get_pixel((bx + dx) as u32, (by + dy) as u32)
                    .0
                    .map(f32::from);
                u += -0.168_736 * r - 0.331_264 * g + 0.5 * b;
                v += 0.5 * r - 0.418_688 * g - 0.081_312 * b;
            }
            let i = w * h + (by / 2) * w + bx;
            out[i] = (v / 4.0 + 128.0).round().clamp(0.0, 255.0) as u8;
            out[i + 1] = (u / 4.0 + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[test]
fn blank_frame_has_no_face() {
    let blank = RgbImage::from_pixel(320, 240, image::Rgb([128, 128, 128]));
    let a = analyze(&rgb_frame(&blank, Rotation::Deg0, false));
    assert_eq!(a.face_count(), 0);
    assert!(a.landmarks.is_none() && a.signals.is_none());
}

#[test]
fn tracking_keeps_the_face_steady() {
    let img = astronaut();
    let frame = rgb_frame(&img, Rotation::Deg0, false);
    let mut analyzer = FaceAnalyzer::new(AnalyzerConfig::default()).unwrap();
    let first = analyzer.analyze(&frame).unwrap();
    // Later frames crop from the previous landmarks instead of the detection.
    let mut last = first.clone();
    for _ in 0..3 {
        last = analyzer.analyze(&frame).unwrap();
    }
    let (mean, _) = distance(landmarks(&last), landmarks(&first));
    eprintln!("tracked vs first frame: mean {mean:.4} px");
    assert!(mean < 0.75, "tracked landmarks drifted {mean:.2} px");
}

#[test]
fn engine_runs_a_session_on_frames() {
    let img = astronaut();
    let mut engine =
        LivenessEngine::new(vec![Challenge::Blink], LivenessConfig::default()).unwrap();
    // A still photo never blinks: the step times out after 10 s.
    let mut status = None;
    for i in 0..=22 {
        let frame = Frame {
            timestamp_ms: i * 500,
            ..rgb_frame(&img, Rotation::Deg0, false)
        };
        let result = engine.process(&frame).unwrap();
        assert_eq!(result.analysis.face_count(), 1);
        assert!(result.actions.is_empty());
        status = Some(result.status);
    }
    assert_eq!(status, Some(Status::Failed(FailReason::StepTimeout)));
}

#[test]
fn counts_two_faces_side_by_side() {
    let face = astronaut();
    let mut pair = RgbImage::new(face.width() * 2, face.height());
    image::imageops::replace(&mut pair, &face, 0, 0);
    image::imageops::replace(&mut pair, &face, face.width() as i64, 0);
    let a = analyze(&rgb_frame(&pair, Rotation::Deg0, false));
    assert_eq!(a.face_count(), 2);
    assert!(a.signals.is_some(), "the primary face is still measured");
}
