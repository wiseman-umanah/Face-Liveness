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


APPROX = ("signals", "real_score", "face_quality")


def check(expected, actual):
    """Exact match, except numbers under `APPROX` keys may differ by the shared tolerance."""
    for key in APPROX:
        if expected.get(key) is not None:
            assert close(expected[key], actual[key], CASES["tolerance"]), (key, expected[key], actual[key])
        else:
            assert actual.get(key) == expected.get(key), key
    assert {k: v for k, v in actual.items() if k not in APPROX} == {
        k: v for k, v in expected.items() if k not in APPROX
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
    face_counts, signals, real_score, result = [], None, None, None
    for i in range(case["frames"]):
        t = i * case["interval_ms"]
        result = engine.process(
            bytes(data), t, width=case["width"], height=case["height"], format=case["format"]
        )
        face_counts.append(result.face_count)
        if i == 0 and result.signals:
            s = result.signals
            signals = {"ear": s.ear, "mouth": s.mouth, "yaw": s.yaw}
            real_score = result.real_score
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
            "real_score": real_score,
            "face_quality": engine.capture.quality if engine.capture else None,
        },
    )


def face_record(case):
    """The bytes a case hands to verify_face_record (one byte flipped if `face_tampered`)."""
    data = bytearray((ROOT / case["face_record"]).read_bytes())
    if case.get("face_tampered"):
        data[100] ^= 1
    return bytes(data)


@pytest.mark.parametrize("case", CASES["receipt_cases"], ids=lambda c: c["name"])
def test_receipt_case(case):
    key = bytes.fromhex(case["public_key_hex"])
    if "face_record" in case:
        verify = lambda: faceliveness.verify_face_record(case["token"], key, face_record(case), now=case["now"])
    else:
        verify = lambda: faceliveness.verify_receipt(case["token"], key, now=case["now"])
    if "expected_error" in case:
        with pytest.raises(ValueError, match=case["expected_error"]):
            verify()
        return
    r = verify()
    check(
        case["expected"],
        {
            "nonce": r.nonce,
            "challenges": r.challenges,
            "passed": r.passed,
            "fail_reason": r.fail_reason,
            "frames": r.frames,
            "frames_sha256": r.frames_sha256,
            "real_score": r.real_score,
            "face_sha256": r.face_sha256,
            "face_quality": r.face_quality,
            "issued_at": r.issued_at,
            "expires_at": r.expires_at,
        },
    )
