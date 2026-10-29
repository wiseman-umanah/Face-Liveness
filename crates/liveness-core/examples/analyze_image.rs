//! Runs face detection + landmarks on a photo.
//!
//!   cargo run -p liveness-core --release --example analyze_image -- <image.png|jpg> [options]
//!
//! Options:
//!   --rotate 90|180|270   treat the image as needing this clockwise rotation to be upright
//!   --mirrored            treat the image as mirrored (selfie view)
//!   --landmarks           print all 478 landmarks as JSON ([[x, y], ...]) instead of a summary

use std::time::Instant;

use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::vision::{AnalyzerConfig, FaceAnalyzer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: analyze_image <image> [--rotate 90|180|270] [--mirrored] [--landmarks]");
        std::process::exit(2);
    };
    let image = image::open(path)
        .unwrap_or_else(|e| fail(&format!("{path}: {e}")))
        .to_rgb8();
    let rotation = match args
        .iter()
        .position(|a| a == "--rotate")
        .map(|i| args.get(i + 1).map(String::as_str))
    {
        None => Rotation::Deg0,
        Some(Some("90")) => Rotation::Deg90,
        Some(Some("180")) => Rotation::Deg180,
        Some(Some("270")) => Rotation::Deg270,
        Some(_) => fail("--rotate expects 90, 180 or 270"),
    };
    let frame = Frame {
        data: image.as_raw(),
        width: image.width(),
        height: image.height(),
        stride: image.width() * 3,
        format: PixelFormat::Rgb8,
        rotation,
        mirrored: args.iter().any(|a| a == "--mirrored"),
        timestamp_ms: 0,
    };

    let start = Instant::now();
    let mut analyzer =
        FaceAnalyzer::new(AnalyzerConfig::default()).unwrap_or_else(|e| fail(&e.to_string()));
    let load_time = start.elapsed();
    let start = Instant::now();
    let analysis = analyzer
        .analyze(&frame)
        .unwrap_or_else(|e| fail(&e.to_string()));
    let first_time = start.elapsed();

    if args.iter().any(|a| a == "--landmarks") {
        let points: Vec<String> = analysis
            .landmarks
            .iter()
            .flatten()
            .map(|p| format!("[{:.3}, {:.3}]", p.x, p.y))
            .collect();
        println!("[{}]", points.join(", "));
        return;
    }

    // Steady-state cost: fresh analyzer state each run so every run includes detection.
    let runs = 10;
    let start = Instant::now();
    for _ in 0..runs {
        analyzer.reset();
        analyzer.analyze(&frame).unwrap();
    }
    let per_frame = start.elapsed() / runs;

    println!("{path}: {}x{}", frame.width, frame.height);
    println!("faces: {}", analysis.faces.len());
    for f in &analysis.faces {
        println!(
            "  box x={:.0} y={:.0} w={:.0} h={:.0} score={:.2}",
            f.x, f.y, f.width, f.height, f.score
        );
    }
    match analysis.signals {
        Some(s) => println!(
            "eye openness (EAR) {:.3}   mouth {:.3}   yaw {:.3}",
            s.ear, s.mouth, s.yaw
        ),
        None => println!("no landmarks"),
    }
    println!(
        "\nmodel load {load_time:.0?}   first frame {first_time:.0?}   per frame {per_frame:.1?}"
    );
    if cfg!(debug_assertions) {
        println!("(debug build: add --release for real numbers)");
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
