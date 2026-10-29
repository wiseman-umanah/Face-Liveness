//! Replays real recordings (see tests/fixtures/README.md) through the detectors.

mod common;

use common::fixture;
use liveness_core::actions::{
    BlinkConfig, BlinkDetector, HeadTurnDetector, MouthOpenDetector, TurnConfig,
};
use liveness_core::challenge::{Challenge, FailReason, SessionConfig, Status};
use liveness_core::session::{LivenessConfig, LivenessSession};
use liveness_core::signals::FaceSignals;

/// (enter, depth) grid from prototype/analyze2.py.
const GRID: [(f32, f32); 8] = [
    (0.7, 0.5),
    (0.7, 0.6),
    (0.8, 0.5),
    (0.8, 0.6),
    (0.8, 0.7),
    (0.85, 0.5),
    (0.85, 0.6),
    (0.85, 0.7),
];

fn blinks(frames: &[(u64, FaceSignals)], config: BlinkConfig) -> usize {
    let mut d = BlinkDetector::new(config);
    frames.iter().filter(|(t, f)| d.update(f.ear, *t)).count()
}

/// The Rust detector must reproduce `python prototype/analyze2.py <file>` exactly.
#[test]
fn blink_counts_match_python_prototype() {
    let expected: [(&str, [usize; 8]); 3] = [
        ("session_01.csv", [5, 12, 3, 9, 12, 3, 9, 11]),
        ("session_02.csv", [2, 11, 2, 11, 11, 2, 10, 10]),
        ("session_03.csv", [0; 8]),
    ];
    for (name, counts) in expected {
        let frames = fixture(name);
        for ((enter, depth), want) in GRID.into_iter().zip(counts) {
            let got = blinks(
                &frames,
                BlinkConfig {
                    enter,
                    depth,
                    ..BlinkConfig::default()
                },
            );
            assert_eq!(got, want, "{name} enter={enter} depth={depth}");
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct Totals {
    blink: usize,
    left: usize,
    right: usize,
    mouth: usize,
}

fn totals(frames: &[(u64, FaceSignals)], turn: TurnConfig) -> Totals {
    let (mut b, mut m, mut h) = (
        BlinkDetector::default(),
        MouthOpenDetector::default(),
        HeadTurnDetector::new(turn),
    );
    let mut out = Totals::default();
    for &(t, f) in frames {
        out.blink += b.update(f.ear, t) as usize;
        out.mouth += m.update(f.mouth, t) as usize;
        match h.update(f.yaw, t) {
            Some(Challenge::TurnLeft) => out.left += 1,
            Some(Challenge::TurnRight) => out.right += 1,
            _ => {}
        }
    }
    out
}

/// Regression lock on default thresholds. The turn and mouth counts match the deliberate
/// episodes visible in the raw data (yaw >=0.4 from baseline, mouth >0.35 held >=300 ms).
/// Update these deliberately when tuning, and say why in the commit.
#[test]
fn default_detector_totals() {
    let t = TurnConfig::default();
    assert_eq!(
        totals(&fixture("session_01.csv"), t),
        Totals {
            blink: 12,
            left: 7,
            right: 3,
            mouth: 11
        }
    );
    assert_eq!(
        totals(&fixture("session_02.csv"), t),
        Totals {
            blink: 11,
            left: 2,
            right: 2,
            mouth: 5
        }
    );
    assert_eq!(
        totals(&fixture("session_03.csv"), t),
        Totals {
            blink: 0,
            left: 3,
            right: 2,
            mouth: 2
        }
    );
}

#[test]
fn mirroring_swaps_turn_direction() {
    let mirrored = TurnConfig {
        mirrored: true,
        ..TurnConfig::default()
    };
    let totals = totals(&fixture("session_03.csv"), mirrored);
    assert_eq!((totals.left, totals.right), (2, 3));
}

fn run(
    frames: &[(u64, FaceSignals)],
    challenges: Vec<Challenge>,
    session: SessionConfig,
) -> (u64, Status) {
    let mut s = LivenessSession::new(
        challenges,
        LivenessConfig {
            session,
            ..LivenessConfig::default()
        },
    )
    .unwrap();
    for &(t, f) in frames {
        match s.update(t, &[f]) {
            Status::InProgress { .. } => {}
            done => return (t, done),
        }
    }
    panic!("recording ended with the session still in progress");
}

#[test]
fn session_passes_on_recorded_actions() {
    let frames = fixture("session_03.csv");
    let from = frames.partition_point(|(t, _)| *t < 25_000);
    let (at, status) = run(
        &frames[from..],
        vec![
            Challenge::TurnLeft,
            Challenge::TurnRight,
            Challenge::OpenMouth,
        ],
        SessionConfig::default(),
    );
    assert_eq!(status, Status::Passed);
    assert_eq!(at, 33_508); // mouth crossed 0.35 at 33_208, held 300 ms
}

#[test]
fn session_fails_when_face_disappears() {
    // session_02 loses the face for 5.7 s at 1:31; with long timeouts that is what ends it.
    let patient = SessionConfig {
        total_timeout_ms: u64::MAX,
        step_timeout_ms: u64::MAX,
        ..SessionConfig::default()
    };
    let (at, status) = run(
        &fixture("session_02.csv"),
        vec![Challenge::Blink; 100],
        patient,
    );
    assert_eq!(status, Status::Failed(FailReason::FaceLost));
    assert!((91_000..98_000).contains(&at), "failed at {at}");
}

#[test]
fn session_times_out_without_the_requested_action() {
    // No blinks are detectable in session_03.
    let (at, status) = run(
        &fixture("session_03.csv"),
        vec![Challenge::Blink],
        SessionConfig::default(),
    );
    assert_eq!(status, Status::Failed(FailReason::StepTimeout));
    assert!(at > 10_000);
}
