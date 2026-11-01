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

#[test]
fn anti_spoofing_matches_reference_on_a_real_photo() {
    // minivision's PyTorch models + crop code on the same face box give 0.8553.
    let a = analyze(&rgb_frame(&astronaut(), Rotation::Deg0, false));
    let score = a.real_score.expect("anti-spoofing runs by default");
    eprintln!("real score {score:.4}");
    assert!((score - 0.8553).abs() < 0.005, "real score {score}");
}

#[cfg(feature = "attest")]
#[test]
fn ticketed_session_produces_a_verifiable_receipt() {
    use liveness_core::attest::SigningKey;

    let now = 1_800_000_000;
    let key = SigningKey::generate().unwrap();
    let (_, token) = key.issue_ticket(vec![Challenge::Blink], now, 120).unwrap();

    // The server verifies the ticket it handed out, then runs the session for it.
    let ticket = key.public_key().verify_ticket(&token, now + 5).unwrap();
    let mut engine =
        LivenessEngine::from_ticket(ticket.clone(), LivenessConfig::default()).unwrap();
    assert!(
        engine.receipt(now + 5, 300).is_err(),
        "no receipt before the session ends"
    );

    let img = astronaut();
    let mut frames = 0;
    loop {
        let frame = Frame {
            timestamp_ms: frames * 500,
            ..rgb_frame(&img, Rotation::Deg0, false)
        };
        frames += 1;
        if engine.process(&frame).unwrap().status
            != (Status::InProgress {
                current: Challenge::Blink,
                remaining: 1,
            })
        {
            break;
        }
    }
    let receipt = engine.receipt(now + 20, 300).unwrap();
    assert_eq!(receipt.nonce, ticket.nonce);
    assert_eq!(receipt.fail_reason, Some(FailReason::StepTimeout)); // a photo can't blink
    assert_eq!(receipt.frames as u64, frames);
    assert!(receipt.real_score.is_some());

    let signed = key.sign_receipt(&receipt).unwrap();
    let verified = key.public_key().verify_receipt(&signed, now + 60).unwrap();
    assert_eq!(verified, receipt);
    assert!(
        SigningKey::generate()
            .unwrap()
            .public_key()
            .verify_receipt(&signed, now + 60)
            .is_err()
    );
    assert!(
        key.public_key()
            .verify_receipt(&signed, now + 20 + 301)
            .is_err(),
        "expired"
    );
    assert!(engine.receipt(now + 121, 300).is_err(), "ticket expired");
}

/// The photo printed on a flat card turned `deg` about its vertical axis, as a pinhole
/// camera would see it (inverse-mapped, bilinear).
fn turned_card(img: &RgbImage, deg: f32) -> RgbImage {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let f = 1.2 * w;
    let (sin, cos) = deg.to_radians().sin_cos();
    RgbImage::from_fn(img.width(), img.height(), |u, v| {
        // Output pixel -> point on the card: solve u' = f X cos / (f + X sin), v' = f Y / (f + X sin).
        let (du, dv) = (u as f32 + 0.5 - w / 2.0, v as f32 + 0.5 - h / 2.0);
        let x = du * f / (f * cos - du * sin);
        let y = dv * (f + x * sin) / f;
        let (sx, sy) = (x + w / 2.0 - 0.5, y + h / 2.0 - 0.5);
        if sx < 0.0 || sy < 0.0 || sx > w - 1.0 || sy > h - 1.0 {
            return image::Rgb([0, 0, 0]);
        }
        let (x0, y0) = (sx.floor() as u32, sy.floor() as u32);
        let (x1, y1) = (
            (x0 + 1).min(img.width() - 1),
            (y0 + 1).min(img.height() - 1),
        );
        let (tx, ty) = (sx.fract(), sy.fract());
        let mix = |c: usize| {
            let p = |x, y| img.get_pixel(x, y)[c] as f32;
            let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
            let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
            (top * (1.0 - ty) + bottom * ty).round() as u8
        };
        image::Rgb([mix(0), mix(1), mix(2)])
    })
}

#[test]
fn tilting_a_photo_cannot_fake_a_head_turn() {
    let img = astronaut();
    // Facing, then the card turned 40 degrees each way, held 1.5 s at a time.
    let poses: Vec<(f32, RgbImage)> = [0.0, 40.0, 0.0, -40.0, 0.0]
        .iter()
        .map(|&d| (d, turned_card(&img, d)))
        .collect();
    for challenge in [Challenge::TurnLeft, Challenge::TurnRight] {
        let mut engine = LivenessEngine::new(vec![challenge], LivenessConfig::default()).unwrap();
        let mut t = 0;
        // 2 s still for the baseline, then the poses at 5 fps.
        for (deg, pose) in std::iter::repeat_n(&poses[0], 10)
            .chain(poses.iter().flat_map(|p| std::iter::repeat_n(p, 8)))
        {
            let r = engine
                .process(&Frame {
                    timestamp_ms: t,
                    ..rgb_frame(pose, Rotation::Deg0, false)
                })
                .unwrap();
            assert!(
                r.actions.is_empty(),
                "{challenge:?}: card at {deg} degrees counted as {:?}",
                r.actions
            );
            t += 200;
        }
    }
    let yaw = |d| {
        analyze(&rgb_frame(&turned_card(&img, d), Rotation::Deg0, false))
            .signals
            .unwrap()
            .yaw
    };
    let (straight, left, right) = (yaw(0.0), yaw(40.0), yaw(-40.0));
    eprintln!("card yaw: 0 deg {straight:.3}, +40 {left:.3}, -40 {right:.3}");
    // A real head turn moves yaw by >= 0.3 (0.4+ in the recordings).
    assert!((left - straight).abs() < 0.1 && (right - straight).abs() < 0.1);
}

#[test]
fn anti_spoofing_runs_every_third_face_frame_by_default() {
    let img = astronaut();
    let frame = rgb_frame(&img, Rotation::Deg0, false);
    let mut analyzer = FaceAnalyzer::new(AnalyzerConfig::default()).unwrap();
    let scored: Vec<bool> = (0..7)
        .map(|_| analyzer.analyze(&frame).unwrap().real_score.is_some())
        .collect();
    assert_eq!(scored, [true, false, false, true, false, false, true]);

    let every = AnalyzerConfig {
        antispoof_every: 1,
        ..AnalyzerConfig::default()
    };
    let mut analyzer = FaceAnalyzer::new(every).unwrap();
    assert!((0..3).all(|_| analyzer.analyze(&frame).unwrap().real_score.is_some()));

    let off = AnalyzerConfig {
        antispoof: false,
        ..AnalyzerConfig::default()
    };
    assert!(
        FaceAnalyzer::new(off)
            .unwrap()
            .analyze(&frame)
            .unwrap()
            .real_score
            .is_none()
    );
}

/// One frame through an engine, returning its face record.
fn face_record(
    img: &RgbImage,
    rotation: Rotation,
    size: u32,
) -> liveness_core::vision::FaceCapture {
    let mut engine =
        LivenessEngine::new(vec![Challenge::Blink], LivenessConfig::default()).unwrap();
    engine.set_capture_size(size);
    engine.process(&rgb_frame(img, rotation, false)).unwrap();
    engine.capture().expect("a face record").clone()
}

#[test]
fn face_record_is_aligned_to_the_standard_template() {
    let capture = face_record(&astronaut(), Rotation::Deg0, 256);
    assert_eq!(capture.size, 256);
    let decoded = image::load_from_memory(&capture.png).unwrap().to_rgb8();
    assert_eq!(
        decoded.as_raw(),
        &capture.rgb,
        "PNG decodes to the raw pixels"
    );

    // Find the face again inside the record. Alignment is a best fit of 5 points (pupils,
    // nose, mouth corners), so each lands within a few pixels of the template; what must hold
    // exactly is level eyes at the template's spacing and position.
    let a = analyze(&rgb_frame(&decoded, Rotation::Deg0, false));
    let p = landmarks(&a);
    let k = 256.0 / 112.0;
    let (left, right) = ((p[468].x, p[468].y), (p[473].x, p[473].y));
    let template = [
        (38.2946 * k + 0.5, 51.6963 * k + 0.5),
        (73.5318 * k + 0.5, 51.5014 * k + 0.5),
    ];
    for ((x, y), (tx, ty)) in [left, right].into_iter().zip(template) {
        let d = ((x - tx).powi(2) + (y - ty).powi(2)).sqrt();
        eprintln!("pupil at ({x:.1}, {y:.1}), template ({tx:.1}, {ty:.1}): {d:.2} px");
        // 8 px here is 3.5 px at the standard 112 size: a typical 5-point fit residual (this
        // face's wide smile pulls the fit; measured 4.8 and 6.1 px).
        assert!(d < 8.0, "pupil {d:.2} px from the template (256 px crop)");
    }
    let tilt = (right.1 - left.1).atan2(right.0 - left.0).to_degrees();
    let spacing = (right.0 - left.0) / (template[1].0 - template[0].0);
    eprintln!("eye line tilt {tilt:.2} degrees, spacing {spacing:.3} x template");
    assert!(tilt.abs() < 2.0, "eyes not level: {tilt:.2} degrees");
    assert!(
        (spacing - 1.0).abs() < 0.08,
        "eye spacing {spacing:.3} x template"
    );
}

#[test]
fn face_record_ignores_camera_rotation() {
    let img = astronaut();
    let upright = face_record(&img, Rotation::Deg0, 112);
    let rotated = face_record(&image::imageops::rotate270(&img), Rotation::Deg90, 112);
    let diff: f64 = upright
        .rgb
        .iter()
        .zip(&rotated.rgb)
        .map(|(a, b)| (*a as f64 - *b as f64).abs())
        .sum::<f64>()
        / upright.rgb.len() as f64;
    eprintln!("rotated vs upright: mean pixel difference {diff:.3}");
    assert!(diff < 1.0);
}

#[test]
fn face_record_quality_drops_for_blur_and_small_faces() {
    let img = astronaut();
    let sharp = face_record(&img, Rotation::Deg0, 112).quality;
    let blurred = face_record(&image::imageops::blur(&img, 3.0), Rotation::Deg0, 112).quality;
    let small = image::imageops::resize(&img, 96, 96, image::imageops::FilterType::Triangle);
    let tiny = face_record(&small, Rotation::Deg0, 112).quality;
    eprintln!("quality: sharp {sharp:.3}, blurred {blurred:.3}, small face {tiny:.3}");
    assert!(sharp > 0.5);
    assert!(blurred < sharp * 0.5);
    assert!(tiny < sharp * 0.5);
}

#[cfg(feature = "attest")]
#[test]
fn receipt_vouches_for_the_face_record() {
    use liveness_core::attest::{SigningKey, sha256_hex};

    let now = 1_800_000_000;
    let key = SigningKey::generate().unwrap();
    let (ticket, _) = key.issue_ticket(vec![Challenge::Blink], now, 120).unwrap();
    let mut engine = LivenessEngine::from_ticket(ticket, LivenessConfig::default()).unwrap();
    let img = astronaut();
    for i in 0..30 {
        let frame = Frame {
            timestamp_ms: i * 500,
            ..rgb_frame(&img, Rotation::Deg0, false)
        };
        if engine.process(&frame).unwrap().status
            != (Status::InProgress {
                current: Challenge::Blink,
                remaining: 1,
            })
        {
            break;
        }
    }
    let capture = engine.capture().unwrap().clone();
    let receipt = engine.receipt(now + 20, 300).unwrap();
    assert_eq!(
        receipt.face_sha256.as_deref(),
        Some(sha256_hex(&capture.png).as_str())
    );
    assert_eq!(receipt.face_quality, Some(capture.quality));

    // A photo can't blink, so the session failed: its face must not be accepted for matching.
    let token = key.sign_receipt(&receipt).unwrap();
    let err = key
        .public_key()
        .verify_face_record(&token, now + 30, &capture.png)
        .unwrap_err();
    assert!(err.to_string().contains("did not pass"), "{err}");
}
