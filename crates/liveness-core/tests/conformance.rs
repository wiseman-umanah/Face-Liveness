//! Runs the shared cases in tests/conformance/cases.json, which the Python and Node bindings
//! also run. Rust is the reference: a case with `"expected": null` (or a mismatch) prints the
//! actual result as JSON to paste in.

mod common;

use std::path::{Path, PathBuf};

use liveness_core::challenge::{Challenge, Status};
use liveness_core::frame::{Frame, PixelFormat, Rotation};
use liveness_core::session::{LivenessConfig, LivenessSession};
use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cases() -> Value {
    let text = std::fs::read_to_string(repo_root().join("tests/conformance/cases.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn config(case: &Value) -> LivenessConfig {
    let mut c = LivenessConfig::default();
    let get = |key: &str| case["config"][key].as_u64();
    if let Some(v) = get("total_timeout_ms") {
        c.session.total_timeout_ms = v;
    }
    if let Some(v) = get("step_timeout_ms") {
        c.session.step_timeout_ms = v;
    }
    if let Some(v) = get("face_lost_ms") {
        c.session.face_lost_ms = v;
    }
    c
}

fn challenges(case: &Value) -> Vec<Challenge> {
    case["challenges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().parse().unwrap())
        .collect()
}

/// (status, fail_reason) as the bindings report them.
fn status_names(status: &Status) -> (&'static str, Option<&'static str>) {
    match status {
        Status::InProgress { .. } => ("in_progress", None),
        Status::Passed => ("passed", None),
        Status::Failed(r) => ("failed", Some(r.as_str())),
    }
}

fn check(kind: &str, case: &Value, actual: Value, failures: &mut Vec<String>) {
    let name = case["name"].as_str().unwrap();
    let expected = &case["expected"];
    let tolerance = cases()["tolerance"].as_f64().unwrap();
    if expected.is_null() || !matches(expected, &actual, tolerance) {
        failures.push(format!(
            "{kind} case {name}: expected {expected}\n  actual: {actual}"
        ));
    }
}

/// Equal JSON, except numbers under a "signals" key may differ by `tolerance`.
fn matches(expected: &Value, actual: &Value, tolerance: f64) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            e.len() == a.len()
                && e.iter().all(|(k, ev)| {
                    a.get(k).is_some_and(|av| {
                        if k == "signals" {
                            close(ev, av, tolerance)
                        } else {
                            matches(ev, av, tolerance)
                        }
                    })
                })
        }
        _ => expected == actual,
    }
}

fn close(expected: &Value, actual: &Value, tolerance: f64) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            e.len() == a.len()
                && e.iter()
                    .all(|(k, ev)| a.get(k).is_some_and(|av| close(ev, av, tolerance)))
        }
        (Value::Number(e), Value::Number(a)) => {
            (e.as_f64().unwrap() - a.as_f64().unwrap()).abs() <= tolerance
        }
        _ => expected == actual,
    }
}

fn run_session_case(case: &Value) -> Value {
    let frames = common::load(repo_root().join(case["recording"].as_str().unwrap()));
    let from = case["from_ms"].as_u64().unwrap();
    let mut session = LivenessSession::new(challenges(case), config(case)).unwrap();
    let mut actions = vec![];
    let mut last = Status::Passed;
    let mut at_ms = Value::Null;
    for &(t, f) in frames.iter().filter(|(t, _)| *t >= from) {
        let (status, detected) = session.observe(t, 1, Some(f));
        actions.extend(detected.iter().map(|a| json!([t, a.as_str()])));
        last = status;
        if !matches!(last, Status::InProgress { .. }) {
            at_ms = json!(t);
            break;
        }
    }
    let (status, fail_reason) = status_names(&last);
    json!({ "status": status, "fail_reason": fail_reason, "at_ms": at_ms, "actions": actions })
}

#[cfg(feature = "vision")]
fn run_engine_case(case: &Value) -> Value {
    use liveness_core::vision::LivenessEngine;

    let mut data = std::fs::read(repo_root().join(case["image"].as_str().unwrap())).unwrap();
    let format: PixelFormat = case["format"].as_str().unwrap().parse().unwrap();
    if format == PixelFormat::Bgr8 {
        data.as_chunks_mut::<3>()
            .0
            .iter_mut()
            .for_each(|p| p.swap(0, 2));
    }
    let (width, height) = (
        case["width"].as_u64().unwrap() as u32,
        case["height"].as_u64().unwrap() as u32,
    );
    let mut engine = LivenessEngine::new(challenges(case), config(case)).unwrap();
    let mut face_counts = vec![];
    let mut first_signals = Value::Null;
    let mut last = Status::Passed;
    let mut at_ms = Value::Null;
    for i in 0..case["frames"].as_u64().unwrap() {
        let t = i * case["interval_ms"].as_u64().unwrap();
        let frame = Frame {
            data: &data,
            width,
            height,
            stride: width * 3,
            format,
            rotation: Rotation::Deg0,
            mirrored: false,
            timestamp_ms: t,
        };
        let r = engine.process(&frame).unwrap();
        face_counts.push(r.analysis.face_count());
        if i == 0
            && let Some(s) = r.analysis.signals
        {
            first_signals = json!({ "ear": s.ear, "mouth": s.mouth, "yaw": s.yaw });
        }
        last = r.status;
        if !matches!(last, Status::InProgress { .. }) {
            at_ms = json!(t);
            break;
        }
    }
    let (status, fail_reason) = status_names(&last);
    json!({
        "status": status,
        "fail_reason": fail_reason,
        "at_ms": at_ms,
        "face_counts": face_counts,
        "signals": first_signals,
    })
}

#[test]
fn shared_conformance_cases() {
    let cases = cases();
    let mut failures = vec![];
    for case in cases["session_cases"].as_array().unwrap() {
        check("session", case, run_session_case(case), &mut failures);
    }
    #[cfg(feature = "vision")]
    for case in cases["engine_cases"].as_array().unwrap() {
        check("engine", case, run_engine_case(case), &mut failures);
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
