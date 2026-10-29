import threading
import time
from pathlib import Path

import numpy as np
import pytest

import faceliveness

ROOT = Path(__file__).resolve().parents[3]


@pytest.fixture(scope="module")
def face():
    """256x256 RGB photo of a face as a NumPy array."""
    return np.fromfile(ROOT / "tests/conformance/astronaut_face.rgb", dtype=np.uint8).reshape(256, 256, 3)


def test_numpy_rgb_and_opencv_bgr_agree(face):
    rgb = faceliveness.Engine(["blink"]).process(face, 0)
    bgr = faceliveness.Engine(["blink"]).process(np.ascontiguousarray(face[:, :, ::-1]), 0, format="bgr")
    assert rgb.face_count == bgr.face_count == 1
    assert rgb.signals.ear == pytest.approx(bgr.signals.ear, abs=1e-6)
    assert len(rgb.landmarks) == 478
    assert rgb.faces[0].score > 0.5


def test_rgba_and_rotation(face):
    rgba = np.dstack([face, np.full(face.shape[:2], 255, np.uint8)])
    upright = faceliveness.Engine(["blink"]).process(face, 0)
    # The buffer is the upright image turned 90 degrees counter-clockwise.
    rotated = faceliveness.Engine(["blink"]).process(np.ascontiguousarray(np.rot90(rgba)), 0, rotation=90)
    assert rotated.signals.ear == pytest.approx(upright.signals.ear, abs=1e-4)


def test_result_fields(face):
    engine = faceliveness.Engine(["turn_left", "blink"])
    assert engine.challenges == ["turn_left", "blink"]
    r = engine.process(face, 0)
    assert (r.status, r.current_challenge, r.remaining, r.fail_reason, r.done) == (
        "in_progress",
        "turn_left",
        2,
        None,
        False,
    )
    assert r.actions == []
    assert "in_progress" in repr(r)


def test_random_engine_uses_os_randomness_unless_seeded():
    seeded = [faceliveness.Engine.random(4, seed=7).challenges for _ in range(2)]
    assert seeded[0] == seeded[1]
    assert all(c in faceliveness.CHALLENGES for c in seeded[0])
    unseeded = {tuple(faceliveness.Session.random(8).challenges) for _ in range(5)}
    assert len(unseeded) > 1


def test_invalid_input_is_reported(face):
    with pytest.raises(ValueError, match="unknown challenge"):
        faceliveness.Engine(["smile"])
    with pytest.raises(ValueError, match="empty"):
        faceliveness.Engine([])
    engine = faceliveness.Engine(["blink"])
    with pytest.raises(ValueError, match="does not match"):
        engine.process(face, 0, format="rgba")
    with pytest.raises(ValueError, match="C-contiguous"):
        engine.process(face[:, ::2], 0)
    with pytest.raises(ValueError, match="uint8"):
        engine.process(face.astype(np.float32), 0)
    with pytest.raises(ValueError, match="rotation"):
        engine.process(face, 0, rotation=45)
    with pytest.raises(ValueError, match="width="):
        engine.process(face.tobytes(), 0)


def test_session_without_images():
    s = faceliveness.Session(["open_mouth"])
    for t in range(0, 300, 50):
        s.update(t, ear=0.3, mouth=0.05, yaw=0.5)
    result = None
    for t in range(300, 800, 50):
        result = s.update(t, ear=0.3, mouth=0.6, yaw=0.5)
        if result.done:
            break
    assert result.status == "passed"
    assert s.update(900, face_count=0).status == "passed"
    with pytest.raises(ValueError, match="all of ear"):
        s.update(1000, ear=0.3)


def test_processing_releases_the_gil(face):
    engine = faceliveness.Engine(["blink"])
    engine.process(face, 0)  # warm up
    ticks = 0
    stop = False

    def count():
        nonlocal ticks
        while not stop:
            ticks += 1
            time.sleep(0)

    t = threading.Thread(target=count)
    t.start()
    for i in range(5):
        engine.process(face, (i + 1) * 100)
    stop = True
    t.join()
    # With the GIL held for the whole inference the counter would barely move.
    assert ticks > 100
