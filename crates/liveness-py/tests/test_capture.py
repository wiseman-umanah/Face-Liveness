import hashlib
import zlib
from pathlib import Path

import numpy as np
import pytest

import faceliveness

ROOT = Path(__file__).resolve().parents[3]
NOW = 1_800_000_000


@pytest.fixture(scope="module")
def face():
    return np.fromfile(ROOT / "tests/conformance/astronaut_face.rgb", dtype=np.uint8).reshape(256, 256, 3)


def test_engine_captures_an_aligned_face_record(face):
    engine = faceliveness.Engine(["blink"])
    assert engine.capture is None
    engine.process(face, 0)
    c = engine.capture
    assert (c.size, c.timestamp_ms) == (112, 0)
    assert 0.5 < c.quality <= 1.0
    assert c.png[:8] == b"\x89PNG\r\n\x1a\n"
    assert c.sha256 == hashlib.sha256(c.png).hexdigest()
    pixels = np.frombuffer(c.rgb, np.uint8).reshape(c.size, c.size, 3)
    assert pixels.std() > 10  # a real image, not blank
    assert "quality=" in repr(c)


def test_capture_size_is_configurable(face):
    engine = faceliveness.Engine(["blink"], capture_size=160)
    engine.process(face, 0)
    assert engine.capture.size == 160
    assert len(engine.capture.rgb) == 160 * 160 * 3


def test_png_decodes_to_the_raw_pixels(face):
    engine = faceliveness.Engine(["blink"])
    engine.process(face, 0)
    c = engine.capture
    # Minimal PNG decode with zlib (8-bit RGB, filter bytes per row) to check it's lossless.
    data, pos, idat = c.png, 8, b""
    while pos < len(data):
        length = int.from_bytes(data[pos : pos + 4], "big")
        kind = data[pos + 4 : pos + 8]
        if kind == b"IDAT":
            idat += data[pos + 8 : pos + 8 + length]
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride = c.size * 3
    rows, prev = [], bytearray(stride)
    for y in range(c.size):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for x in range(stride):
            a = line[x - 3] if x >= 3 else 0
            b, cc = prev[x], prev[x - 3] if x >= 3 else 0
            if f == 1: line[x] = (line[x] + a) & 255
            elif f == 2: line[x] = (line[x] + b) & 255
            elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - cc
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - cc)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else cc)) & 255
        rows.append(bytes(line))
        prev = line
    assert b"".join(rows) == c.rgb


def test_receipt_vouches_for_the_face_record_but_not_after_a_failure(face):
    key = faceliveness.SigningKey.generate()
    ticket = key.issue_ticket(["blink"], now=NOW)
    engine = faceliveness.Engine.from_ticket(ticket, key.public_key, now=NOW)
    result = None
    for i in range(30):  # a still photo can't blink: the step times out
        result = engine.process(face, i * 500)
        if result.done:
            break
    receipt = engine.receipt(key, now=NOW + 20)
    r = faceliveness.verify_receipt(receipt, key.public_key, now=NOW + 30)
    assert r.face_sha256 == engine.capture.sha256
    assert r.face_quality == pytest.approx(engine.capture.quality)
    with pytest.raises(ValueError, match="did not pass"):
        faceliveness.verify_face_record(receipt, key.public_key, engine.capture.png, now=NOW + 30)
