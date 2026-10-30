from pathlib import Path

import numpy as np
import pytest

import faceliveness

ROOT = Path(__file__).resolve().parents[3]
NOW = 1_800_000_000


@pytest.fixture(scope="module")
def face():
    return np.fromfile(ROOT / "tests/conformance/astronaut_face.rgb", dtype=np.uint8).reshape(256, 256, 3)


def test_key_round_trip():
    key = faceliveness.SigningKey.generate()
    assert len(key.to_bytes()) == len(key.public_key) == 32
    again = faceliveness.SigningKey.from_bytes(key.to_bytes())
    assert again.public_key == key.public_key
    assert "secret" in repr(key) and key.to_bytes().hex() not in repr(key)
    with pytest.raises(ValueError, match="32 bytes"):
        faceliveness.SigningKey.from_bytes(b"short")


def test_ticket_issue_and_verify():
    key = faceliveness.SigningKey.generate()
    token = key.issue_ticket(["blink", "turn_left"], ttl_secs=60, now=NOW)
    t = faceliveness.verify_ticket(token, key.public_key, now=NOW + 30)
    assert t.challenges == ["blink", "turn_left"]
    assert (t.issued_at, t.expires_at, len(t.nonce)) == (NOW, NOW + 60, 32)
    assert len(faceliveness.verify_ticket(key.issue_ticket(count=4, now=NOW), key.public_key, now=NOW).challenges) == 4
    with pytest.raises(ValueError, match="expired"):
        faceliveness.verify_ticket(token, key.public_key, now=NOW + 61)
    with pytest.raises(ValueError, match="signature"):
        faceliveness.verify_ticket(token, faceliveness.SigningKey.generate().public_key, now=NOW)


def test_ticketed_session_end_to_end(face):
    key = faceliveness.SigningKey.generate()
    token = key.issue_ticket(["open_mouth"], ttl_secs=120, now=NOW)
    engine = faceliveness.Engine.from_ticket(token, key.public_key, now=NOW + 1)
    assert engine.challenges == ["open_mouth"]
    with pytest.raises(ValueError, match="not finished"):
        engine.receipt(key, now=NOW + 1)

    result, scores = None, []
    for i in range(30):  # a still photo can't open its mouth: times out after 10 s
        result = engine.process(face, i * 500)
        scores.append(result.real_score)
        if result.done:
            break
    # Anti-spoofing runs on every 3rd frame with a face.
    assert [s is not None for s in scores[:4]] == [True, False, False, True]
    assert result.fail_reason == "step_timeout"

    receipt_token = engine.receipt(key, now=NOW + 20)
    r = faceliveness.verify_receipt(receipt_token, key.public_key, now=NOW + 30)
    assert not r.passed and r.fail_reason == "step_timeout"
    assert r.frames == i + 1 and len(r.frames_sha256) == 64
    assert r.nonce == faceliveness.verify_ticket(token, key.public_key, now=NOW).nonce
    assert 0.0 <= r.real_score <= 1.0
    with pytest.raises(ValueError, match="ticket"):
        faceliveness.verify_ticket(receipt_token, key.public_key, now=NOW + 30)


def test_from_ticket_rejects_forged_tickets():
    key, attacker = faceliveness.SigningKey.generate(), faceliveness.SigningKey.generate()
    forged = attacker.issue_ticket(["blink"], now=NOW)
    with pytest.raises(ValueError, match="signature"):
        faceliveness.Engine.from_ticket(forged, key.public_key, now=NOW)


def test_engine_accepts_min_real_score():
    # The policy itself is tested in the Rust core (session::tests); this checks the option is wired.
    assert faceliveness.Engine(["blink"], min_real_score=0.99).challenges == ["blink"]
    assert faceliveness.Engine.random(2, min_real_score=0.5).challenges
