//! Replays a recording from `prototype/probe.py` through the Rust detectors.
//!
//!   cargo run -p liveness-core --release --example replay -- <file.csv> [options]
//!
//! Options:
//!   --challenges blink,mouth,left,right   also run a liveness session against the recording
//!   --from <seconds>                      start the session this far in (detectors need ~1.5 s
//!                                         of frames to learn the baseline before they fire)
//!   --mirrored                            frames were mirrored (selfie view)
//!   --sweep                               blink counts across a grid of thresholds
//!   --quiet                               hide the per-event timeline

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::Instant;

use liveness_core::actions::{
    BlinkConfig, BlinkDetector, HeadTurnDetector, MouthOpenDetector, TurnConfig,
};
use liveness_core::challenge::{Challenge, SessionConfig, Status};
use liveness_core::session::{LivenessConfig, LivenessSession};
use liveness_core::signals::FaceSignals;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args
        .iter()
        .find(|a| !a.starts_with("--") && !is_value_of_flag(&args, a))
    else {
        eprintln!(
            "usage: replay <file.csv> [--challenges blink,mouth,left,right] [--from <seconds>] [--mirrored] [--sweep] [--quiet]"
        );
        std::process::exit(2);
    };
    let frames = common::load(path);
    if frames.is_empty() {
        eprintln!("{path}: no frames");
        std::process::exit(1);
    }
    let mirrored = args.iter().any(|a| a == "--mirrored");
    let quiet = args.iter().any(|a| a == "--quiet");
    let turn_config = TurnConfig {
        mirrored,
        ..TurnConfig::default()
    };

    summary(path, &frames);
    detect(&frames, turn_config, quiet);
    if let Some(list) = flag_value(&args, "--challenges") {
        let from_ms = flag_value(&args, "--from").map_or(0, |v| {
            let secs: f64 = v.parse().unwrap_or_else(|_| {
                eprintln!("--from expects seconds, got {v:?}");
                std::process::exit(2);
            });
            (secs * 1000.0) as u64
        });
        let start = frames.partition_point(|(t, _)| *t < from_ms);
        run_session(&frames[start..], parse_challenges(list), turn_config);
    }
    if args.iter().any(|a| a == "--sweep") {
        sweep(&frames);
    }
    timing(&frames, turn_config);
}

fn summary(path: &str, frames: &[(u64, FaceSignals)]) {
    let first = frames[0].0;
    let last = frames[frames.len() - 1].0;
    let secs = (last - first) as f64 / 1000.0;
    let gaps: Vec<(u64, u64)> = frames
        .windows(2)
        .filter(|w| w[1].0 - w[0].0 > 1000)
        .map(|w| (w[0].0, w[1].0 - w[0].0))
        .collect();
    println!("== {path}");
    println!(
        "frames with a face: {}   duration: {secs:.1}s   ~{:.1} fps",
        frames.len(),
        frames.len() as f64 / secs
    );
    for (at, len) in gaps {
        println!(
            "face missing for {:.1}s starting at {}",
            len as f64 / 1000.0,
            clock(at)
        );
    }
}

fn detect(frames: &[(u64, FaceSignals)], turn_config: TurnConfig, quiet: bool) {
    let mut blink = BlinkDetector::default();
    let mut mouth = MouthOpenDetector::default();
    let mut turn = HeadTurnDetector::new(turn_config);
    let mut counts = [0usize; 4];

    if !quiet {
        println!("\n-- detected actions");
    }
    for &(t, f) in frames {
        let mut events = vec![];
        if blink.update(f.ear, t) {
            events.push(Challenge::Blink);
        }
        if mouth.update(f.mouth, t) {
            events.push(Challenge::OpenMouth);
        }
        if let Some(dir) = turn.update(f.yaw, t) {
            events.push(dir);
        }
        for e in events {
            counts[Challenge::ALL.iter().position(|c| *c == e).unwrap()] += 1;
            if !quiet {
                println!("{}  {}", clock(t), name(e));
            }
        }
    }
    println!("\n-- totals");
    for (c, n) in Challenge::ALL.iter().zip(counts) {
        println!("{:<11} {n}", name(*c));
    }
}

fn run_session(frames: &[(u64, FaceSignals)], challenges: Vec<Challenge>, turn_config: TurnConfig) {
    let names: Vec<&str> = challenges.iter().map(|c| name(*c)).collect();
    println!("\n-- session: {}", names.join(" -> "));
    if frames.is_empty() {
        println!("no frames after --from");
        return;
    }
    let config = LivenessConfig {
        turn: turn_config,
        ..LivenessConfig::default()
    };
    let mut session = LivenessSession::new(challenges, config).expect("valid challenge list");
    let mut previous_remaining = usize::MAX;
    for &(t, f) in frames {
        match session.update(t, &[f]) {
            Status::InProgress { current, remaining } => {
                if remaining != previous_remaining {
                    println!("{}  waiting for {}", clock(t), name(current));
                    previous_remaining = remaining;
                }
            }
            Status::Passed => {
                println!("{}  PASSED", clock(t));
                return;
            }
            Status::Failed(reason) => {
                println!("{}  FAILED: {reason:?}", clock(t));
                return;
            }
        }
    }
    println!("recording ended before the session finished");
}

fn sweep(frames: &[(u64, FaceSignals)]) {
    println!("\n-- blink sweep (enter x baseline, depth x baseline)");
    println!("enter  depth  blinks");
    for enter in [0.7, 0.8, 0.85] {
        for depth in [0.5, 0.6, 0.7] {
            if depth >= enter {
                continue;
            }
            let mut d = BlinkDetector::new(BlinkConfig {
                enter,
                depth,
                ..BlinkConfig::default()
            });
            let n = frames.iter().filter(|(t, f)| d.update(f.ear, *t)).count();
            println!("{enter:.2}   {depth:.2}   {n}");
        }
    }
}

/// Runs every detector over the recording repeatedly and reports cost per frame.
fn timing(frames: &[(u64, FaceSignals)], turn_config: TurnConfig) {
    let runs = (200_000 / frames.len()).max(1);
    let start = Instant::now();
    // No timeouts and a challenge list that never completes, so every frame runs every detector.
    let never_end = SessionConfig {
        total_timeout_ms: u64::MAX,
        step_timeout_ms: u64::MAX,
        face_lost_ms: u64::MAX,
        multiple_faces_ms: u64::MAX,
    };
    let config = LivenessConfig {
        session: never_end,
        turn: turn_config,
        ..Default::default()
    };
    let mut fired = 0usize;
    for _ in 0..runs {
        let mut session = LivenessSession::new(vec![Challenge::Blink; 1_000], config).unwrap();
        for &(t, f) in frames {
            fired += session.update_with_actions(t, &[f]).1.len();
        }
    }
    let per_frame = start.elapsed().as_secs_f64() / (runs * frames.len()) as f64;
    std::hint::black_box(fired);
    println!("\n-- speed (decision logic only, excludes landmark detection)");
    println!(
        "{:.2} µs/frame   ~{:.0} frames/s on one core",
        per_frame * 1e6,
        1.0 / per_frame
    );
    if cfg!(debug_assertions) {
        println!("(debug build: add --release for real numbers)");
    }
}

fn parse_challenges(list: &str) -> Vec<Challenge> {
    list.split(',')
        .map(|s| match s.trim() {
            "blink" => Challenge::Blink,
            "mouth" => Challenge::OpenMouth,
            "left" => Challenge::TurnLeft,
            "right" => Challenge::TurnRight,
            other => {
                eprintln!("unknown challenge {other:?}; use blink, mouth, left, right");
                std::process::exit(2);
            }
        })
        .collect()
}

fn name(c: Challenge) -> &'static str {
    match c {
        Challenge::Blink => "blink",
        Challenge::OpenMouth => "mouth open",
        Challenge::TurnLeft => "turn left",
        Challenge::TurnRight => "turn right",
    }
}

fn clock(ms: u64) -> String {
    format!(
        "{:>3}:{:02}.{:01}",
        ms / 60_000,
        ms / 1000 % 60,
        ms % 1000 / 100
    )
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn is_value_of_flag(args: &[String], arg: &str) -> bool {
    ["--challenges", "--from"]
        .iter()
        .any(|flag| flag_value(args, flag).is_some_and(|v| std::ptr::eq(v, arg)))
}
