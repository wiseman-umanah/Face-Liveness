"""Runs the shared cases in tests/conformance/cases.json (the Rust core and the Node
binding run the same file) and checks Python gets identical results."""

import csv
import json
from pathlib import Path

import pytest

import faceliveness

ROOT = Path(__file__).resolve().parents[3]
CASES = json.loads((ROOT / "tests/conformance/cases.json").read_text())


def close(expected, actual, tol):
    if isinstance(expected, dict):
        return expected.keys() == actual.keys() and all(close(expected[k], actual[k], tol) for k in expected)
    return abs(expected - actual) <= tol


def check(expected, actual):
    signals = expected.get("signals")
    if signals is not None:
        assert close(signals, actual["signals"], CASES["tolerance"]), (signals, actual["signals"])
    assert {k: v for k, v in actual.items() if k != "signals"} == {
        k: v for k, v in expected.items() if k != "signals"
    }


@pytest.mark.parametrize("case", CASES["session_cases"], ids=lambda c: c["name"])
def test_session_case(case):
    session = faceliveness.Session(case["challenges"], **case["config"])
    actions, result = [], None
    with open(ROOT / case["recording"]) as f:
        for row in csv.DictReader(f):
            t = int(row["ts_ms"])
            if t < case["from_ms"]:
                continue
            result = session.update(t, face_count=1, ear=float(row["ear"]), mouth=float(row["mouth"]), yaw=float(row["yaw"]))
            actions += [[t, a] for a in result.actions]
            if result.done:
                break
    check(
        case["expected"],
        {
            "status": result.status,
            "fail_reason": result.fail_reason,
            "at_ms": t if result.done else None,
            "actions": actions,
        },
    )


@pytest.mark.parametrize("case", CASES["engine_cases"], ids=lambda c: c["name"])
def test_engine_case(case):
    data = bytearray((ROOT / case["image"]).read_bytes())
    if case["format"] == "bgr":
        data[0::3], data[2::3] = data[2::3], data[0::3]
    engine = faceliveness.Engine(case["challenges"], **case["config"])
    face_counts, signals, result = [], None, None
    for i in range(case["frames"]):
        t = i * case["interval_ms"]
        result = engine.process(
            bytes(data), t, width=case["width"], height=case["height"], format=case["format"]
        )
        face_counts.append(result.face_count)
        if i == 0 and result.signals:
            s = result.signals
            signals = {"ear": s.ear, "mouth": s.mouth, "yaw": s.yaw}
        if result.done:
            break
    check(
        case["expected"],
        {
            "status": result.status,
            "fail_reason": result.fail_reason,
            "at_ms": t if result.done else None,
            "face_counts": face_counts,
            "signals": signals,
        },
    )
