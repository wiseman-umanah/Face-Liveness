//! Loads recordings made by `prototype/probe.py` (columns: ts_ms, ear, mouth, yaw).
//! Only frames with a face are recorded, so gaps in ts_ms are frames without one.

use std::path::Path;

use liveness_core::signals::FaceSignals;

pub fn load(path: impl AsRef<Path>) -> Vec<(u64, FaceSignals)> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.split(',').collect();
            let num = |i: usize| -> f32 {
                cols[i]
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| panic!("bad row: {line}"))
            };
            let ts = cols[0]
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("bad row: {line}"));
            (
                ts,
                FaceSignals {
                    ear: num(1),
                    mouth: num(2),
                    yaw: num(3),
                },
            )
        })
        .collect()
}

#[allow(dead_code)]
pub fn fixture(name: &str) -> Vec<(u64, FaceSignals)> {
    load(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
}
