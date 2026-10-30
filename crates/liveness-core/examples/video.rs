//! Runs the full Rust pipeline on video piped in from ffmpeg as raw RGB frames.
//!
//! Video file:
//!   ffmpeg -loglevel error -i clip.mp4 -f rawvideo -pix_fmt rgb24 - \
//!     | cargo run -p liveness-core --release --example video -- --size 640x480 --fps 30
//!
//! Webcam (Linux), live liveness check:
//!   ffmpeg -loglevel error -f v4l2 -video_size 640x480 -i /dev/video0 -vf fps=15 \
//!       -f rawvideo -pix_fmt rgb24 - \
//!     | cargo run -p liveness-core --release --example video -- --size 640x480 --live \
//!       --challenges blink,left,right,mouth
//!
//! Options:
//!   --size WxH            frame size (required; must match ffmpeg's output)
//!   --fps N               timestamps = frame index / N (default 30; ignored with --live)
//!   --live                timestamps from the wall clock (for cameras)
//!   --csv out.csv         write ts_ms,ear,mouth,yaw,real_score per frame with a face
//!                         (probe.py's columns plus the anti-spoofing score),
//!                         replayable with the `replay` example
//!   --challenges list     run a liveness session: blink,mouth,left,right
//!   --random N            run a session of N random challenges instead
//!   --mirrored            frames are mirrored (selfie view)
//!   --min-real-score X    fail a completed session if the mean anti-spoofing score is below X

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::time::{Duration, Instant, SystemTime};

use liveness_core::challenge::{Challenge, Status};
use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::session::{LivenessConfig, LivenessSession};
use liveness_core::vision::{AnalyzerConfig, FaceAnalyzer, LivenessEngine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };

    let (width, height) = value("--size")
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .unwrap_or_else(|| fail("--size WxH is required (e.g. --size 640x480)"));
    let fps: f64 = value("--fps").map_or(30.0, |v| {
        v.parse().unwrap_or_else(|_| fail("--fps expects a number"))
    });
    let live = flag("--live");
    let mirrored = flag("--mirrored");
    let mut csv = value("--csv").map(|path| {
        let mut f =
            BufWriter::new(File::create(path).unwrap_or_else(|e| fail(&format!("{path}: {e}"))));
        writeln!(f, "ts_ms,ear,mouth,yaw,real_score").unwrap();
        f
    });

    let analyzer =
        FaceAnalyzer::new(AnalyzerConfig::default()).unwrap_or_else(|e| fail(&e.to_string()));
    let challenges = match (value("--challenges"), value("--random")) {
        (Some(list), _) => Some(parse_challenges(list)),
        (None, Some(n)) => {
            let n = n
                .parse()
                .unwrap_or_else(|_| fail("--random expects a count"));
            // Demo only: real deployments must take the seed from a server-side CSPRNG.
            let seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            Some(
                LivenessSession::random(n, seed, LivenessConfig::default())
                    .unwrap()
                    .challenges()
                    .to_vec(),
            )
        }
        (None, None) => None,
    };
    let min_real_score = value("--min-real-score").map(|v| {
        v.parse::<f32>()
            .unwrap_or_else(|_| fail("--min-real-score expects a number"))
    });
    let session_config = if challenges.is_some() {
        LivenessConfig {
            min_real_score,
            ..LivenessConfig::default()
        }
    } else {
        // Measurement only: a session that never ends and never times out.
        let mut c = LivenessConfig::default();
        c.session.total_timeout_ms = u64::MAX;
        c.session.step_timeout_ms = u64::MAX;
        c.session.face_lost_ms = u64::MAX;
        c.session.multiple_faces_ms = u64::MAX;
        c
    };
    let session_challenges = challenges
        .clone()
        .unwrap_or_else(|| vec![Challenge::Blink; 1_000_000]);
    let mut engine = LivenessEngine::from_parts(
        analyzer,
        LivenessSession::new(session_challenges, session_config).unwrap(),
    );
    if let Some(list) = &challenges {
        let names: Vec<&str> = list.iter().map(|c| name(*c)).collect();
        eprintln!("session: {}", names.join(" -> "));
    }

    let mut buf = vec![0u8; (width * height * 3) as usize];
    let mut stdin = std::io::stdin().lock();
    let started = Instant::now();
    let mut times: Vec<Duration> = vec![];
    let mut real_scores: Vec<f32> = vec![];
    let (mut frames, mut with_face) = (0u64, 0u64);
    let mut last_prompt: Option<Challenge> = None;
    let mut outcome: Option<Status> = None;

    while stdin.read_exact(&mut buf).is_ok() {
        let timestamp_ms = if live {
            started.elapsed().as_millis() as u64
        } else {
            (frames as f64 * 1000.0 / fps) as u64
        };
        frames += 1;
        let frame = Frame {
            data: &buf,
            width,
            height,
            stride: width * 3,
            format: PixelFormat::Rgb8,
            rotation: Rotation::Deg0,
            mirrored,
            timestamp_ms,
        };
        let t = Instant::now();
        let result = engine
            .process(&frame)
            .unwrap_or_else(|e| fail(&e.to_string()));
        times.push(t.elapsed());

        let real = result.analysis.real_score;
        real_scores.extend(real);
        if let Some(s) = result.analysis.signals {
            with_face += 1;
            if let Some(f) = csv.as_mut() {
                let real = real.map_or(String::new(), |r| format!("{r:.4}"));
                writeln!(
                    f,
                    "{timestamp_ms},{:.3},{:.3},{:.3},{real}",
                    s.ear, s.mouth, s.yaw
                )
                .unwrap();
            }
        }
        for a in &result.actions {
            eprintln!("\r{}  detected {:<40}", clock(timestamp_ms), name(*a));
        }
        if live {
            let face = match (result.analysis.face_count(), result.analysis.signals) {
                (0, _) => "no face".to_string(),
                (n, _) if n > 1 => format!("{n} faces"),
                (_, Some(s)) => format!(
                    "EAR {:.2}  mouth {:.2}  yaw {:.2}  real {:.2}",
                    s.ear,
                    s.mouth,
                    s.yaw,
                    real.unwrap_or(f32::NAN)
                ),
                (_, None) => "face".to_string(),
            };
            eprint!("\r{}  {face:<52}", clock(timestamp_ms));
        }
        if challenges.is_some() {
            match result.status {
                Status::InProgress { current, .. } if last_prompt != Some(current) => {
                    eprintln!(
                        "\r{}  >>> please: {:<30}",
                        clock(timestamp_ms),
                        prompt(current)
                    );
                    last_prompt = Some(current);
                }
                Status::InProgress { .. } => {}
                done => {
                    eprintln!(
                        "\r{}  {:<40}",
                        clock(timestamp_ms),
                        format!("{done:?}").to_uppercase()
                    );
                    outcome = Some(done);
                    break;
                }
            }
        }
    }

    if let Some(mut f) = csv {
        f.flush().unwrap();
    }
    if frames == 0 {
        fail("no frames read: check --size matches ffmpeg's output");
    }
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let mean = times.iter().map(|d| ms(*d)).sum::<f64>() / times.len() as f64;
    eprintln!(
        "\n-- {frames} frames, face measured in {with_face} ({:.0}%)",
        100.0 * with_face as f64 / frames as f64
    );
    eprintln!(
        "per frame: mean {mean:.1} ms   median {:.1} ms   p95 {:.1} ms   -> up to ~{:.0} fps",
        ms(times[times.len() / 2]),
        ms(times[times.len() * 95 / 100]),
        1000.0 / mean
    );
    if !real_scores.is_empty() {
        let mut sorted = real_scores.clone();
        sorted.sort_by(f32::total_cmp);
        let mean = sorted.iter().sum::<f32>() / sorted.len() as f32;
        eprintln!(
            "anti-spoofing real score: mean {mean:.3}   p5 {:.3}   median {:.3}   p95 {:.3}",
            sorted[sorted.len() * 5 / 100],
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 95 / 100]
        );
    }
    if challenges.is_some() && outcome.is_none() {
        eprintln!("video ended before the session finished");
    }
    if cfg!(debug_assertions) {
        eprintln!("(debug build: add --release for real numbers)");
    }
}

fn parse_challenges(list: &str) -> Vec<Challenge> {
    list.split(',')
        .map(|s| match s.trim() {
            "blink" => Challenge::Blink,
            "mouth" => Challenge::OpenMouth,
            "left" => Challenge::TurnLeft,
            "right" => Challenge::TurnRight,
            other => fail(&format!(
                "unknown challenge {other:?}; use blink, mouth, left, right"
            )),
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

fn prompt(c: Challenge) -> &'static str {
    match c {
        Challenge::Blink => "blink",
        Challenge::OpenMouth => "open your mouth wide",
        Challenge::TurnLeft => "turn your head to your left",
        Challenge::TurnRight => "turn your head to your right",
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

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
