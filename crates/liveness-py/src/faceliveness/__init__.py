"""On-device face liveness checks.

    import faceliveness

    engine = faceliveness.Engine.random(3)          # e.g. blink -> turn_left -> open_mouth
    for frame, ts in camera_frames():               # HxWx3 uint8 arrays, timestamps in ms
        result = engine.process(frame, ts, format="bgr")   # "bgr" for OpenCV frames
        if result.done:
            print(result.status, result.fail_reason)
            break
        print("please:", result.current_challenge)
"""

from __future__ import annotations

from typing import Any, Iterable, Literal, Optional

from faceliveness import _core
from faceliveness._core import (
    FaceBox,
    FaceCapture,
    FrameResult,
    Receipt,
    Session,
    Signals,
    SigningKey,
    Ticket,
    verify_face_record,
    verify_receipt,
    verify_ticket,
    version,
)

__all__ = [
    "CHALLENGES",
    "Challenge",
    "Engine",
    "FaceBox",
    "FaceCapture",
    "FrameResult",
    "PixelFormat",
    "Receipt",
    "Session",
    "Signals",
    "SigningKey",
    "Ticket",
    "verify_face_record",
    "verify_receipt",
    "verify_ticket",
    "version",
]

Challenge = Literal["blink", "turn_left", "turn_right", "open_mouth"]
PixelFormat = Literal["rgb", "bgr", "rgba", "bgra", "nv21"]
CHALLENGES: tuple[Challenge, ...] = ("blink", "turn_left", "turn_right", "open_mouth")

_CHANNELS = {"rgb": 3, "bgr": 3, "rgba": 4, "bgra": 4}


class Engine:
    """Runs face detection, landmarks and the challenge flow on camera frames.

    Each engine is one liveness session for one person; create a new one per attempt.
    Model loading takes a few hundred milliseconds.
    """

    def __init__(
        self,
        challenges: Iterable[Challenge],
        *,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
        min_real_score: Optional[float] = None,
        capture_size: int = 112,
    ) -> None:
        """min_real_score: if set, a session that completes its challenges still fails with
        "spoof_suspected" when the mean anti-spoofing score is below it. Off by default:
        calibrate on your own cameras first (see FrameResult.real_score)."""
        self._engine = _core.Engine(
            list(challenges),
            total_timeout_ms=total_timeout_ms,
            step_timeout_ms=step_timeout_ms,
            face_lost_ms=face_lost_ms,
            min_real_score=min_real_score,
        )
        self._engine.set_capture_size(capture_size)

    @classmethod
    def random(
        cls,
        count: int,
        *,
        seed: Optional[int] = None,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
        min_real_score: Optional[float] = None,
        capture_size: int = 112,
    ) -> "Engine":
        """`count` challenges in random order. Leave `seed` unset in production: it then
        comes from the OS, so users can't predict (and pre-record) the challenges."""
        engine = cls.__new__(cls)
        engine._engine = _core.Engine.random(
            count,
            seed=seed,
            total_timeout_ms=total_timeout_ms,
            step_timeout_ms=step_timeout_ms,
            face_lost_ms=face_lost_ms,
            min_real_score=min_real_score,
        )
        engine._engine.set_capture_size(capture_size)
        return engine

    @classmethod
    def from_ticket(
        cls,
        token: str,
        public_key: bytes,
        *,
        now: Optional[int] = None,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
        min_real_score: Optional[float] = None,
        capture_size: int = 112,
    ) -> "Engine":
        """A session for a ticket from `SigningKey.issue_ticket`. Raises ValueError if the
        ticket is forged, signed by another key, or expired. Every frame is hashed into the
        receipt; `now` is Unix seconds (default: the system clock)."""
        engine = cls.__new__(cls)
        engine._engine = _core.Engine.from_ticket(
            token,
            public_key,
            now=now,
            total_timeout_ms=total_timeout_ms,
            step_timeout_ms=step_timeout_ms,
            face_lost_ms=face_lost_ms,
            min_real_score=min_real_score,
        )
        engine._engine.set_capture_size(capture_size)
        return engine

    @property
    def capture(self) -> Optional[FaceCapture]:
        """The best face of the session so far, aligned for face matching (`capture_size`
        pixels square, default 112), or None if no single face was seen. Hand `capture.png`
        to your matching service together with the receipt; it checks both with
        `verify_face_record`. As a NumPy array:
        `np.frombuffer(c.rgb, np.uint8).reshape(c.size, c.size, 3)`."""
        return self._engine.capture

    def receipt(self, signing_key: SigningKey, *, ttl_secs: int = 300, now: Optional[int] = None) -> str:
        """Signed receipt (JWT) for a finished ticketed session, for other services to check
        with `verify_receipt`. Raises ValueError if the session isn't finished or the ticket
        has expired."""
        return self._engine.receipt(signing_key, ttl_secs=ttl_secs, now=now)

    @property
    def challenges(self) -> list[Challenge]:
        return self._engine.challenges

    def process(
        self,
        frame: Any,
        timestamp_ms: int,
        *,
        format: Optional[PixelFormat] = None,
        rotation: int = 0,
        mirrored: bool = False,
        width: Optional[int] = None,
        height: Optional[int] = None,
        stride: Optional[int] = None,
    ) -> FrameResult:
        """Processes one frame.

        frame: a uint8 array of shape (height, width, 3|4), e.g. from NumPy or OpenCV, or a
            (height * 3 // 2, width) array for NV21. Raw bytes also work if you pass
            `width`, `height` and `format`.
        timestamp_ms: capture time in milliseconds; drives every timeout.
        format: "rgb" (default for 3 channels), "bgr" (OpenCV), "rgba" (default for 4
            channels), "bgra" or "nv21".
        rotation: clockwise degrees (0, 90, 180, 270) that turn the frame upright.
        mirrored: True for selfie-style mirrored frames.
        """
        data, width, height, stride, format = _frame_layout(frame, format, width, height, stride)
        return self._engine.process_raw(
            data, width, height, stride, format, rotation, mirrored, timestamp_ms
        )


def _frame_layout(frame, format, width, height, stride):
    view = memoryview(frame)
    if view.itemsize != 1:
        raise ValueError(f"frame must hold uint8 pixels, got item size {view.itemsize}")

    if view.ndim == 1:
        if width is None or height is None or format is None:
            raise ValueError("raw byte frames need width=, height= and format=")
    elif view.ndim == 2 and format == "nv21":
        rows, width = view.shape
        height = rows * 2 // 3
    elif view.ndim == 3:
        height, width, channels = view.shape
        if format is None:
            format = {3: "rgb", 4: "rgba"}.get(channels)
            if format is None:
                raise ValueError(f"frame must have 3 or 4 channels, got {channels}")
        elif _CHANNELS.get(format) != channels:
            raise ValueError(f"format {format!r} does not match a frame with {channels} channels")
    else:
        raise ValueError(
            f"expected a (height, width, channels) array, got shape {tuple(view.shape)}"
        )

    if view.ndim > 1:
        if not view.c_contiguous:
            raise ValueError("frame must be C-contiguous; use numpy.ascontiguousarray(frame)")
        stride = view.strides[0]
    elif stride is None:
        stride = width * _CHANNELS.get(format, 1)
    return view.tobytes(), width, height, stride, format
