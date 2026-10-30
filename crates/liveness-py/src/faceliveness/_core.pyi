from typing import Literal, Optional

_Status = Literal["in_progress", "passed", "failed"]
_Challenge = Literal["blink", "turn_left", "turn_right", "open_mouth"]
_FailReason = Literal["timeout", "step_timeout", "face_lost", "multiple_faces", "spoof_suspected"]

def version() -> str: ...

class Signals:
    """Eye, mouth and head-turn measurements of one face."""

    @property
    def ear(self) -> float:
        """Eye aspect ratio: ~0.25-0.35 open, drops sharply during a blink."""
    @property
    def mouth(self) -> float:
        """Inner-lip height / width: ~0.05 closed, >0.4 wide open."""
    @property
    def yaw(self) -> float:
        """Nose position between the face edges: ~0.5 facing the camera."""

class FaceBox:
    """A detected face in upright pixel coordinates."""

    @property
    def x(self) -> float: ...
    @property
    def y(self) -> float: ...
    @property
    def width(self) -> float: ...
    @property
    def height(self) -> float: ...
    @property
    def score(self) -> float: ...

class FrameResult:
    """Outcome of one frame."""

    @property
    def status(self) -> _Status: ...
    @property
    def done(self) -> bool:
        """True once passed or failed; later frames won't change the outcome."""
    @property
    def current_challenge(self) -> Optional[_Challenge]:
        """What the user should do now, while in progress."""
    @property
    def remaining(self) -> int:
        """Challenges left including the current one, while in progress."""
    @property
    def fail_reason(self) -> Optional[_FailReason]: ...
    @property
    def actions(self) -> list[_Challenge]:
        """Actions recognised in this frame."""
    @property
    def face_count(self) -> int: ...
    @property
    def signals(self) -> Optional[Signals]: ...
    @property
    def faces(self) -> list[FaceBox]: ...
    @property
    def landmarks(self) -> Optional[list[tuple[float, float]]]:
        """478 face-mesh landmarks of the primary face, in upright pixels."""
    @property
    def real_score(self) -> Optional[float]:
        """Passive anti-spoofing: probability (0-1) that the face is real rather than a photo
        or screen. Measured on every 3rd frame with a face (None otherwise); uncalibrated for
        your cameras."""

class Engine:
    def __init__(
        self,
        challenges: list[str],
        *,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
        min_real_score: Optional[float] = None,
    ) -> None: ...
    @staticmethod
    def random(
        count: int,
        *,
        seed: Optional[int] = None,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
        min_real_score: Optional[float] = None,
    ) -> Engine: ...
    @staticmethod
    def from_ticket(
        token: str,
        public_key: bytes,
        *,
        now: Optional[int] = None,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
        min_real_score: Optional[float] = None,
    ) -> Engine: ...
    def receipt(self, signing_key: SigningKey, *, ttl_secs: int = 300, now: Optional[int] = None) -> str: ...
    @property
    def challenges(self) -> list[_Challenge]: ...
    def process_raw(
        self,
        data: bytes,
        width: int,
        height: int,
        stride: int,
        format: str,
        rotation: int,
        mirrored: bool,
        timestamp_ms: int,
    ) -> FrameResult: ...

class Session:
    """Liveness decisions from measurements you compute yourself (no image processing)."""

    def __init__(
        self,
        challenges: list[str],
        *,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
    ) -> None: ...
    @staticmethod
    def random(
        count: int,
        *,
        seed: Optional[int] = None,
        total_timeout_ms: Optional[int] = None,
        step_timeout_ms: Optional[int] = None,
        face_lost_ms: Optional[int] = None,
    ) -> Session: ...
    @property
    def challenges(self) -> list[_Challenge]: ...
    def update(
        self,
        timestamp_ms: int,
        *,
        face_count: int = 1,
        ear: Optional[float] = None,
        mouth: Optional[float] = None,
        yaw: Optional[float] = None,
    ) -> FrameResult: ...

class SigningKey:
    """Ed25519 key for issuing tickets and signing receipts. Keep it on your server."""

    @staticmethod
    def generate() -> SigningKey: ...
    @staticmethod
    def from_bytes(secret: bytes) -> SigningKey: ...
    def to_bytes(self) -> bytes:
        """The 32-byte secret: store it like a password."""
    @property
    def public_key(self) -> bytes:
        """The 32-byte public key, for verifying tickets and receipts."""
    def issue_ticket(
        self,
        challenges: Optional[list[str]] = None,
        *,
        count: int = 3,
        ttl_secs: int = 120,
        now: Optional[int] = None,
    ) -> str:
        """A signed ticket (JWT) for `challenges`, or for `count` random ones if not given."""

class Ticket:
    @property
    def nonce(self) -> str: ...
    @property
    def challenges(self) -> list[_Challenge]: ...
    @property
    def issued_at(self) -> int: ...
    @property
    def expires_at(self) -> int: ...

class Receipt:
    @property
    def nonce(self) -> str:
        """The ticket's nonce: accept each nonce only once."""
    @property
    def challenges(self) -> list[_Challenge]: ...
    @property
    def passed(self) -> bool: ...
    @property
    def fail_reason(self) -> Optional[_FailReason]: ...
    @property
    def frames(self) -> int: ...
    @property
    def frames_sha256(self) -> str: ...
    @property
    def real_score(self) -> Optional[float]: ...
    @property
    def issued_at(self) -> int: ...
    @property
    def expires_at(self) -> int: ...

def verify_ticket(token: str, public_key: bytes, *, now: Optional[int] = None) -> Ticket:
    """Checks a ticket's signature and expiry; raises ValueError if invalid."""

def verify_receipt(token: str, public_key: bytes, *, now: Optional[int] = None) -> Receipt:
    """Checks a receipt's signature and expiry; raises ValueError if invalid."""
