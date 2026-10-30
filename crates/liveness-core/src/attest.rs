//! Signed challenge tickets and liveness receipts.
//!
//! Trust model: a liveness result computed on the user's device can't be trusted, however it
//! is signed, because a key shipped to the device can be extracted. So the signing key lives on
//! your server, and the flow is:
//!
//! 1. The server issues a **ticket** (random nonce, challenge order, expiry) and sends it to
//!    the client. The client can't choose easier challenges or reuse an old ticket.
//! 2. The server runs [`LivenessEngine::from_ticket`](crate::vision::LivenessEngine::from_ticket)
//!    on the frames sent for that ticket.
//! 3. The server signs a **receipt** (outcome, nonce, a SHA-256 hash chain over every processed
//!    frame) that other services verify with only the public key.
//!
//! Tickets and receipts are standard JWTs signed with Ed25519 (`alg: EdDSA`), so any JWT
//! library can verify them too. Times are Unix seconds, as JWT requires. Verifiers must also
//! reject a nonce they have already accepted; that needs storage, so it's left to the caller.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Signer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::challenge::{Challenge, ChallengeSession, FailReason, SessionConfig};
use crate::error::{LivenessError, Result};
use crate::frame::Frame;

const ISSUER: &str = "faceliveness";
const HEADER: &str = r#"{"alg":"EdDSA","typ":"JWT"}"#;
/// Tolerated clock difference between issuer and verifier.
const CLOCK_SKEW_SECS: u64 = 60;

fn invalid(msg: impl Into<String>) -> LivenessError {
    LivenessError::InvalidToken(msg.into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Ed25519 private key: keep it on the server.
#[derive(Clone)]
pub struct SigningKey(ed25519_dalek::SigningKey);

/// Ed25519 public key: share it with every service that verifies tickets or receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(ed25519_dalek::VerifyingKey);

impl SigningKey {
    /// A new random key from the OS.
    pub fn generate() -> Result<Self> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|e| invalid(format!("no OS randomness: {e}")))?;
        Ok(Self::from_bytes(&secret))
    }

    pub fn from_bytes(secret: &[u8; 32]) -> Self {
        Self(ed25519_dalek::SigningKey::from_bytes(secret))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.verifying_key())
    }

    /// Issues a ticket for `challenges`, valid for `ttl_secs` from `now` (Unix seconds).
    pub fn issue_ticket(
        &self,
        challenges: Vec<Challenge>,
        now: u64,
        ttl_secs: u64,
    ) -> Result<(Ticket, String)> {
        if challenges.is_empty() {
            return Err(LivenessError::InvalidConfig(
                "challenge list is empty".into(),
            ));
        }
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| invalid(format!("no OS randomness: {e}")))?;
        let ticket = Ticket {
            nonce: hex(&nonce),
            challenges,
            issued_at: now,
            expires_at: now + ttl_secs,
        };
        let token = self.sign(&ticket.claims())?;
        Ok((ticket, token))
    }

    /// Issues a ticket for `count` challenges in an order drawn from OS randomness.
    pub fn issue_random_ticket(
        &self,
        count: usize,
        now: u64,
        ttl_secs: u64,
    ) -> Result<(Ticket, String)> {
        let seed = getrandom::u64().map_err(|e| invalid(format!("no OS randomness: {e}")))?;
        let challenges = ChallengeSession::random(count, seed, SessionConfig::default())?
            .challenges()
            .to_vec();
        self.issue_ticket(challenges, now, ttl_secs)
    }

    pub fn sign_receipt(&self, receipt: &Receipt) -> Result<String> {
        self.sign(&receipt.claims())
    }

    fn sign(&self, claims: &Claims) -> Result<String> {
        let payload = serde_json::to_vec(claims).map_err(|e| invalid(e.to_string()))?;
        let signing_input = format!("{}.{}", B64.encode(HEADER), B64.encode(payload));
        let signature = self.0.sign(signing_input.as_bytes());
        Ok(format!(
            "{signing_input}.{}",
            B64.encode(signature.to_bytes())
        ))
    }
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SigningKey").field(&"<redacted>").finish()
    }
}

impl PublicKey {
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        ed25519_dalek::VerifyingKey::from_bytes(bytes)
            .map(Self)
            .map_err(|_| invalid("not a valid Ed25519 public key"))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Checks a ticket's signature, kind and expiry.
    pub fn verify_ticket(&self, token: &str, now: u64) -> Result<Ticket> {
        let claims = self.verify(token, "ticket", now)?;
        Ok(Ticket {
            nonce: claims.jti,
            challenges: parse_challenges(&claims.challenges)?,
            issued_at: claims.iat,
            expires_at: claims.exp,
        })
    }

    /// Checks a receipt's signature, kind and expiry.
    pub fn verify_receipt(&self, token: &str, now: u64) -> Result<Receipt> {
        let claims = self.verify(token, "receipt", now)?;
        let missing = |field: &str| invalid(format!("receipt has no {field}"));
        let status = claims.status.ok_or_else(|| missing("status"))?;
        let fail_reason = match (status.as_str(), claims.fail_reason) {
            ("passed", None) => None,
            ("failed", Some(r)) => Some(parse_fail_reason(&r)?),
            _ => return Err(invalid("inconsistent status and fail_reason")),
        };
        Ok(Receipt {
            nonce: claims.jti,
            challenges: parse_challenges(&claims.challenges)?,
            fail_reason,
            frames: claims.frames.ok_or_else(|| missing("frames"))?,
            frames_sha256: claims
                .frames_sha256
                .ok_or_else(|| missing("frames_sha256"))?,
            real_score: claims.real_score,
            issued_at: claims.iat,
            expires_at: claims.exp,
        })
    }

    fn verify(&self, token: &str, kind: &str, now: u64) -> Result<Claims> {
        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid("not a JWT"));
        };
        // Only EdDSA is accepted, so "alg: none" or algorithm-confusion tokens fail here.
        let header: serde_json::Value = decode_json(header)?;
        if header["alg"] != "EdDSA" {
            return Err(invalid("token must be signed with EdDSA"));
        }
        let signature = B64
            .decode(signature)
            .map_err(|_| invalid("bad signature encoding"))?;
        let signature =
            Signature::from_slice(&signature).map_err(|_| invalid("bad signature length"))?;
        let signing_input = &token[..token.len() - token.rsplit('.').next().unwrap().len() - 1];
        self.0
            .verify_strict(signing_input.as_bytes(), &signature)
            .map_err(|_| invalid("signature does not match"))?;

        let claims: Claims = decode_json(payload)?;
        if claims.iss != ISSUER || claims.kind != kind {
            return Err(invalid(format!("not a faceliveness {kind}")));
        }
        if now > claims.exp {
            return Err(invalid(format!("{kind} expired")));
        }
        // Issued in the future: the issuer's clock or the token is wrong.
        if claims.iat > now + CLOCK_SKEW_SECS {
            return Err(invalid(format!("{kind} is not valid yet")));
        }
        Ok(claims)
    }
}

fn decode_json<T: serde::de::DeserializeOwned>(part: &str) -> Result<T> {
    let bytes = B64.decode(part).map_err(|_| invalid("bad base64"))?;
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("bad JSON: {e}")))
}

fn parse_challenges(names: &[String]) -> Result<Vec<Challenge>> {
    names.iter().map(|n| n.parse()).collect()
}

fn parse_fail_reason(name: &str) -> Result<FailReason> {
    FailReason::ALL
        .into_iter()
        .find(|r| r.as_str() == name)
        .ok_or_else(|| invalid(format!("unknown fail_reason {name:?}")))
}

/// What the server asks for, and until when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// 32 hex characters; unique per ticket.
    pub nonce: String,
    pub challenges: Vec<Challenge>,
    pub issued_at: u64,
    pub expires_at: u64,
}

impl Ticket {
    fn claims(&self) -> Claims {
        Claims {
            iss: ISSUER.into(),
            kind: "ticket".into(),
            jti: self.nonce.clone(),
            iat: self.issued_at,
            exp: self.expires_at,
            challenges: self.challenges.iter().map(|c| c.as_str().into()).collect(),
            status: None,
            fail_reason: None,
            frames: None,
            frames_sha256: None,
            real_score: None,
        }
    }
}

/// The outcome of a ticketed session, as signed by the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The ticket's nonce: check it is one you issued and haven't accepted before.
    pub nonce: String,
    pub challenges: Vec<Challenge>,
    /// None if the session passed.
    pub fail_reason: Option<FailReason>,
    /// Frames processed.
    pub frames: u32,
    /// Hex SHA-256 chain over every processed frame (see [`FrameDigest`]), for audits.
    pub frames_sha256: String,
    /// Mean passive anti-spoofing "real" probability, if it was measured.
    pub real_score: Option<f32>,
    pub issued_at: u64,
    pub expires_at: u64,
}

impl Receipt {
    pub fn passed(&self) -> bool {
        self.fail_reason.is_none()
    }

    fn claims(&self) -> Claims {
        Claims {
            iss: ISSUER.into(),
            kind: "receipt".into(),
            jti: self.nonce.clone(),
            iat: self.issued_at,
            exp: self.expires_at,
            challenges: self.challenges.iter().map(|c| c.as_str().into()).collect(),
            status: Some(if self.passed() { "passed" } else { "failed" }.into()),
            fail_reason: self.fail_reason.map(|r| r.as_str().into()),
            frames: Some(self.frames),
            frames_sha256: Some(self.frames_sha256.clone()),
            real_score: self.real_score,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    kind: String,
    jti: String,
    iat: u64,
    exp: u64,
    challenges: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fail_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frames: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frames_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    real_score: Option<f32>,
}

/// SHA-256 chain over frames: `h0 = sha256(nonce)`, `h(n) = sha256(h(n-1) || frame n)`, where
/// a frame contributes its timestamp, size, layout and pixels. Anyone holding the same frames
/// can recompute it, so stored footage can later be matched to a receipt.
#[derive(Debug, Clone)]
pub struct FrameDigest {
    state: [u8; 32],
    frames: u32,
}

impl FrameDigest {
    pub fn new(nonce: &str) -> Self {
        Self {
            state: Sha256::digest(nonce.as_bytes()).into(),
            frames: 0,
        }
    }

    pub fn update(&mut self, frame: &Frame) {
        let mut h = Sha256::new();
        h.update(self.state);
        h.update(frame.timestamp_ms.to_le_bytes());
        for v in [frame.width, frame.height, frame.stride] {
            h.update(v.to_le_bytes());
        }
        h.update(frame.format.as_str().as_bytes());
        h.update([frame.rotation as u8, frame.mirrored as u8]);
        h.update(frame.data);
        self.state = h.finalize().into();
        self.frames += 1;
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn hex(&self) -> String {
        hex(&self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn receipt(nonce: &str) -> Receipt {
        Receipt {
            nonce: nonce.into(),
            challenges: vec![Challenge::Blink],
            fail_reason: None,
            frames: 42,
            frames_sha256: "ab".repeat(32),
            real_score: Some(0.9),
            issued_at: NOW,
            expires_at: NOW + 300,
        }
    }

    #[test]
    fn ticket_round_trip() {
        let (ticket, token) = key()
            .issue_ticket(vec![Challenge::Blink, Challenge::TurnLeft], NOW, 120)
            .unwrap();
        assert_eq!(ticket.nonce.len(), 32);
        assert_eq!(
            key().public_key().verify_ticket(&token, NOW + 60).unwrap(),
            ticket
        );
    }

    #[test]
    fn random_tickets_have_fresh_nonces() {
        let (a, _) = key().issue_random_ticket(3, NOW, 60).unwrap();
        let (b, _) = key().issue_random_ticket(3, NOW, 60).unwrap();
        assert_ne!(a.nonce, b.nonce);
        assert_eq!(a.challenges.len(), 3);
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let (_, token) = key()
            .issue_ticket(vec![Challenge::Blink], NOW, 120)
            .unwrap();
        let err = key()
            .public_key()
            .verify_ticket(&token, NOW + 121)
            .unwrap_err();
        assert!(err.to_string().contains("expired"), "{err}");
    }

    #[test]
    fn token_from_the_future_is_rejected() {
        let (_, token) = key()
            .issue_ticket(vec![Challenge::Blink], NOW, 600)
            .unwrap();
        assert!(
            key().public_key().verify_ticket(&token, NOW - 60).is_ok(),
            "within clock skew"
        );
        let err = key()
            .public_key()
            .verify_ticket(&token, NOW - 61)
            .unwrap_err();
        assert!(err.to_string().contains("not valid yet"), "{err}");
    }

    #[test]
    fn wrong_key_and_tampering_are_rejected() {
        let (_, token) = key()
            .issue_ticket(vec![Challenge::TurnLeft], NOW, 120)
            .unwrap();
        let other = SigningKey::from_bytes(&[8; 32]).public_key();
        assert!(other.verify_ticket(&token, NOW).is_err());

        // Swap the payload for one asking for an easier challenge, keeping the signature.
        let (_, easier) = SigningKey::from_bytes(&[9; 32])
            .issue_ticket(vec![Challenge::Blink], NOW, 120)
            .unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let forged = format!(
            "{}.{}.{}",
            parts[0],
            easier.split('.').nth(1).unwrap(),
            parts[2]
        );
        assert!(key().public_key().verify_ticket(&forged, NOW).is_err());
    }

    #[test]
    fn alg_none_and_garbage_are_rejected() {
        let payload = B64.encode(r#"{"iss":"faceliveness","kind":"ticket","jti":"x","iat":0,"exp":9999999999,"challenges":["blink"]}"#);
        let none = format!("{}.{payload}.", B64.encode(r#"{"alg":"none","typ":"JWT"}"#));
        let pk = key().public_key();
        assert!(pk.verify_ticket(&none, NOW).is_err());
        for junk in ["", "a.b", "a.b.c", "a.b.c.d"] {
            assert!(pk.verify_ticket(junk, NOW).is_err(), "{junk:?}");
        }
    }

    #[test]
    fn ticket_is_not_accepted_as_receipt_or_vice_versa() {
        let k = key();
        let (_, ticket) = k.issue_ticket(vec![Challenge::Blink], NOW, 120).unwrap();
        let receipt = k.sign_receipt(&receipt("n")).unwrap();
        assert!(k.public_key().verify_receipt(&ticket, NOW).is_err());
        assert!(k.public_key().verify_ticket(&receipt, NOW).is_err());
    }

    #[test]
    fn receipt_round_trip_and_failed_outcome() {
        let k = key();
        let r = receipt("abc");
        assert_eq!(
            k.public_key()
                .verify_receipt(&k.sign_receipt(&r).unwrap(), NOW)
                .unwrap(),
            r
        );
        let failed = Receipt {
            fail_reason: Some(FailReason::FaceLost),
            real_score: None,
            ..receipt("abc")
        };
        let back = k
            .public_key()
            .verify_receipt(&k.sign_receipt(&failed).unwrap(), NOW)
            .unwrap();
        assert!(!back.passed());
        assert_eq!(back, failed);
    }

    #[test]
    fn signatures_are_deterministic() {
        // Ed25519 signatures depend only on key and message, so other languages can check
        // they produce byte-identical tokens.
        let r = receipt("abc");
        assert_eq!(
            key().sign_receipt(&r).unwrap(),
            key().sign_receipt(&r).unwrap()
        );
    }

    #[test]
    fn key_bytes_round_trip_and_debug_hides_secret() {
        let k = SigningKey::generate().unwrap();
        assert_eq!(
            SigningKey::from_bytes(&k.to_bytes()).public_key(),
            k.public_key()
        );
        assert_eq!(
            PublicKey::from_bytes(&k.public_key().to_bytes()).unwrap(),
            k.public_key()
        );
        assert!(!format!("{k:?}").contains(&format!("{:?}", k.to_bytes())));
    }

    #[test]
    fn frame_digest_depends_on_nonce_pixels_and_order() {
        use crate::frame::{PixelFormat, Rotation};
        let frame = |data: &'static [u8], ts| Frame {
            data,
            width: 1,
            height: 1,
            stride: 3,
            format: PixelFormat::Rgb8,
            rotation: Rotation::Deg0,
            mirrored: false,
            timestamp_ms: ts,
        };
        let digest = |nonce: &str, frames: &[Frame]| {
            let mut d = FrameDigest::new(nonce);
            frames.iter().for_each(|f| d.update(f));
            d.hex()
        };
        let (a, b) = (frame(&[1, 2, 3], 0), frame(&[1, 2, 4], 33));
        let base = digest("n", &[a, b]);
        assert_eq!(base, digest("n", &[a, b]));
        assert_ne!(base, digest("m", &[a, b]));
        assert_ne!(base, digest("n", &[b, a]));
        assert_ne!(base, digest("n", &[a, frame(&[1, 2, 5], 33)]));
        assert_eq!(base.len(), 64);
    }
}
