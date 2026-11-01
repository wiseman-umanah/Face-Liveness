# Face matching integration guide

For whoever builds face matching (enrolment and 1:1 verification) on top of Face Liveness. It
covers what this SDK hands you, how to trust it, and what is left for the matching side.

## What this SDK does and doesn't do

**Does (ready for an MVP):**

- A liveness check: the user performs random actions (blink, turn left, turn right, open
  mouth) in front of the camera, with presence rules (face lost, multiple faces) and timeouts.
- A **face record**: the session's best frame (most frontal, eyes open, mouth closed, sharp,
  well lit, big enough), aligned for face recognition and encoded as PNG.
- A **signed receipt** (Ed25519 JWT) recording the outcome and the SHA-256 of that exact PNG.
- `verify_face_record` / `verifyFaceRecord`: the one call the matching side uses to accept a
  face only if it came from a passed liveness session.

**Doesn't (your side):**

- No face recognition: no embeddings, no comparison, no "same person?" decision.
- No storage of faces or templates.
- No ID-document checks.

## The flow

```
client (camera)                 liveness service (server)                  matching service
---------------                 -------------------------                  ----------------
                    <-- ticket  key.issue_ticket()  (server picks challenges)
frames ----------------------->  Engine.from_ticket(ticket, public_key)
                                 engine.process(frame, ts) ... until done
                                 receipt  = engine.receipt(key)
                                 face_png = engine.capture.png
                                 ----- receipt + face_png ------------------->  verify_face_record(receipt, public_key, face_png)
                                                                                 reject a nonce seen before
                                                                                 enrol:  embedding(face_png) -> store
                                                                                 verify: embedding(face_png) vs stored -> score
```

Run the liveness engine on a **server**, not on the user's device: a result computed on the
device can be faked. The signing key stays on the liveness server; the matching service only
needs the **public key**.

## Install (not yet on PyPI/npm)

Both packages build a native Rust extension, so the machine needs a Rust toolchain
(https://rustup.rs) the first time.

```bash
# Python 3.9+ (from a local checkout)
uv add /path/to/face-liveness/crates/liveness-py        # or: pip install /path/to/face-liveness/crates/liveness-py
# or from GitHub (needs access to the repo)
pip install "git+ssh://git@github.com/wiseman-umanah/Face-Liveness.git#subdirectory=crates/liveness-py"

# Node 22+ (build the addon first, then install the folder)
cd /path/to/face-liveness/crates/liveness-node && pnpm install && pnpm build
npm install /path/to/face-liveness/crates/liveness-node
```

Python imports as `faceliveness`; the npm package is also named `faceliveness`.

## Liveness service

### Python

```python
import faceliveness

key = faceliveness.SigningKey.from_bytes(SECRET_32_BYTES)   # load from your secret store;
                                                             # create once with SigningKey.generate().to_bytes()
PUBLIC_KEY = key.public_key                                  # 32 bytes: give this to the matching service

# 1. Start a session: the ticket fixes the challenges and expires (default 120 s).
ticket = key.issue_ticket(count=3)                           # send to the client; it shows the prompts

# 2. Run the engine on the frames the client streams (use your server's clock for timestamps).
engine = faceliveness.Engine.from_ticket(ticket, key.public_key)   # optional: capture_size=112
result = engine.process(frame, timestamp_ms, format="bgr")         # NumPy/OpenCV frame; "rgb", "rgba", ...
# result.current_challenge -> what to ask the user for; repeat until result.done

# 3. Hand over the outcome.
receipt = engine.receipt(key)            # signed JWT, valid 300 s by default (ttl_secs=)
face_png = engine.capture.png            # bytes; None-safe: engine.capture is None if no face was seen
```

`engine.process` releases the GIL; a session's engine is not meant to be shared between
threads at the same time. Create a new engine per attempt (model loading takes ~0.5 s).

### Node

```ts
import { Engine, SigningKey } from 'faceliveness'

const key = SigningKey.fromBytes(SECRET_32_BYTES)
const ticket = key.issueTicket({ count: 3 })
const engine = Engine.fromTicket(ticket, key.publicKey)        // optional: { captureSize: 112 }
const result = await engine.processAsync(frame, { width, height, timestampMs, format: 'rgba' })
// ... until result.done
const receipt = engine.receipt(key)
const facePng = engine.capture()?.png                          // Buffer
```

`demo/server.mjs` in this repo is a complete, dependency-free reference server for this flow.

## Matching service

Before enrolling or comparing **any** face:

```python
import faceliveness

try:
    r = faceliveness.verify_face_record(receipt, PUBLIC_KEY, face_png)
except ValueError as e:
    reject(str(e))   # forged/expired receipt, session did not pass, no face, or face_png was swapped
if nonce_already_used(r.nonce):      # store accepted nonces; a receipt must work only once
    reject("replayed receipt")
mark_nonce_used(r.nonce)
# Now face_png is a face from a live, passed session. Proceed to enrol or compare.
```

```ts
import { verifyFaceRecord } from 'faceliveness'
const r = verifyFaceRecord(receipt, PUBLIC_KEY, facePng)   // throws with the same reasons
```

`verify_face_record` fails with one of: `signature does not match`, `receipt expired`,
`... is not valid yet`, `the liveness session did not pass`, `receipt has no face record`,
`face record does not match the receipt`. Receipts are standard EdDSA JWTs, so a service in
another language can verify them with any JWT library and compare `face_sha256` with the
SHA-256 of the PNG bytes itself.

### Receipt fields

| Field (Python / Node) | Meaning |
|---|---|
| `nonce` | Unique per session: accept each only once |
| `passed`, `fail_reason` / `failReason` | Outcome; reasons: `timeout`, `step_timeout`, `face_lost`, `multiple_faces`, `spoof_suspected` |
| `challenges` | What the user was asked to do |
| `face_sha256` / `faceSha256` | SHA-256 (hex) of the face record PNG bytes |
| `face_quality` / `faceQuality` | Quality of the face record, 0-1 |
| `real_score` / `realScore` | Passive anti-spoofing mean (uncalibrated; informational) |
| `frames`, `frames_sha256` / `framesSha256` | Frames judged and a hash chain over them (audits) |
| `issued_at`, `expires_at` / `issuedAt`, `expiresAt` | Unix seconds |

## The face record

| Property | Value |
|---|---|
| Format | PNG, 8-bit **RGB** (no alpha), lossless, deterministic |
| Size | 112 x 112 by default (`capture_size` / `captureSize`, 32-512) |
| Alignment | Similarity transform of 5 points (pupils, nose tip, mouth corners) onto the standard ArcFace template: at 112 x 112 the eyes sit near (38.3, 51.7) and (73.5, 51.5), nose (56.0, 71.7), mouth corners (41.5, 92.4) and (70.7, 92.2) |
| Orientation | Upright and un-mirrored (camera rotation and selfie mirroring are undone) |
| Also available | `rgb` raw pixels (size x size x 3), `quality`, `timestamp_ms`, `sha256` |

Practical notes for the matcher:

- **Feed it straight to the recognition model**: most models (ArcFace-family, SFace,
  MobileFaceNet) expect exactly this 112 x 112 aligned crop, so don't detect or re-align again.
  Apply the model's own normalisation (e.g. `(pixel - 127.5) / 128`).
- **Colour order**: the PNG is RGB. `cv2.imdecode` returns **BGR**; convert if the model wants
  RGB. In Python without OpenCV: `np.frombuffer(capture.rgb, np.uint8).reshape(s, s, 3)`.
- **Store the PNG bytes unchanged** if you want to re-verify later; re-encoding changes the hash.
- **Quality**: below ~0.3 matching will be unreliable; ask the user to retry rather than enrol
  a poor face. The score is a heuristic that hasn't been tuned against a real matcher yet; check
  your model's scores against it and adjust the cut-off.

## Choosing the recognition model (matching side)

- **Licence first**: for a commercial (fintech) product the weights must allow commercial use.
  The popular InsightFace/ArcFace weights are **non-commercial only**. A likely candidate is
  OpenCV Zoo's SFace (Apache-2.0); verify the licence of whatever you pick.
- **Threshold**: compare embeddings with cosine similarity and choose the accept threshold from
  measured error rates: FAR (strangers accepted) vs FRR (owners rejected). Fintech targets are
  typically FAR <= 1 in 10,000 or stricter.
- **Templates are biometric data**: encrypt at rest, keep them server-side, support deletion,
  get explicit consent, and check the local law (e.g. GDPR, Nigeria Data Protection Act 2023).

## Known limits of the liveness side (production gaps)

- Not published to PyPI/npm yet; install from source.
- Passive anti-spoofing (`real_score`) is not calibrated, so it's report-only: phone-screen
  replays are not reliably caught yet. Printed photos are defeated by the challenges.
- Not defended: live deepfakes, injected virtual cameras, 3D masks.
- No accuracy testing across different people, lighting and devices yet; blinks are unreliable
  with glasses.
- Speed: ~50-60 ms/frame natively, ~75-95 ms in the browser on a 2013 laptop CPU.

Details: [threat-model.md](threat-model.md).

## Reference material in this repo

- `demo/server.mjs`: working liveness service + a `/api/verify` that does exactly the matching
  side's acceptance check.
- `tests/conformance/cases.json`: fixed receipts and face records with expected verification
  results, usable as test vectors for any implementation.
- `crates/liveness-py/README.md`, `crates/liveness-node/README.md`: full API.
