# Threat model

What Face Liveness defends against, what it doesn't, and the evidence. "Tested" means a test
in this repository checks it; "untested" means it's a design argument only.

## What a check consists of

1. **Challenge-response.** The user performs a random sequence of actions (blink, turn left,
   turn right, open mouth), each within a time limit. Actions are measured relative to the
   user's own baseline, so they are hard to fake without a live, moving face.
2. **Presence rules.** The session fails if the face disappears for more than 1 s or a second
   face stays in view for more than 0.5 s.
3. **Passive anti-spoofing (report-only by default).** Two MiniFASNet models score each frame
   for print/screen texture (`real_score`, 0-1). Setting `min_real_score` makes a low mean
   score fail the session; it is off until you calibrate it (see below).
4. **Face record.** The session's best frame (frontal, eyes open, sharp, well lit) is aligned
   for face matching and its hash goes into the receipt, so liveness and the face to match are
   bound together.
5. **Server-side trust (optional).** Your server signs a ticket fixing the challenges and an
   expiry, runs the engine on the frames, and signs a receipt with the outcome and a SHA-256
   chain over every frame. Receipts are standard Ed25519 JWTs.

## Attacks

| Attack | Status | Evidence |
|---|---|---|
| Printed photo held to the camera | **Defended** by challenges | A photo can't blink or open its mouth. Turning a flat photo doesn't register as a head turn: a card turned ±40° moves `yaw` by ≤0.05, a real turn needs ≥0.3 (*tested*: `tilting_a_photo_cannot_fake_a_head_turn`; ±45° on the full photo moved it ≤0.016) |
| Photo with cut-out eyes/mouth, or a paper mask | Partly | A blink through cut-out eyes could pass; head turns still can't (the face is flat). Passive anti-spoofing may flag the print, *untested* |
| Pre-recorded video of the victim on a screen | Partly | The challenge order comes from OS randomness and each step has a 10 s limit, so one recording rarely matches (*tested*: seeding, timeouts). An attacker switching between clips of each action live can pass the challenges; passive anti-spoofing is the defense there and **must be calibrated** before it is enforced |
| Real-time deepfake / face swap, virtual camera, injected video stream | **Not defended** | Needs camera/device attestation or injection detection, which this project doesn't provide |
| High-quality 3D mask | **Not defended** | MiniFASNet isn't evaluated against masks |
| Client claims "passed" without doing the check | **Defended** with server-side tickets and receipts | Results only count when your server ran the engine and signed the receipt (*tested*: wrong key, tampering, `alg: none`, ticket/receipt confusion, expiry and future-dated tokens are all rejected) |
| Client sends fake timestamps (e.g. slow motion to stretch time limits) | **Defended** in the demo server | `demo/server.mjs` timestamps frames with the server's clock and ignores client times; do the same in your backend |
| Browser (on-device) result reported as "passed" | **Not trustworthy by design** | On-device mode is for feedback and demos; the demo labels it "not tamper-proof". Use server-verified mode for decisions |
| Pass liveness with your own face, then submit someone else's photo for face matching | **Defended** with `verify_face_record` | The face record is captured from the live session and its SHA-256 is in the signed receipt; the matcher accepts only that exact image from a passed session (*tested*: tampered image, failed session, missing face all rejected, in every language) |
| Client picks easier challenges or reuses an old ticket | **Defended** | Tickets are signed and expire (default 120 s) |
| Replaying a receipt | Defended **if you track nonces** | Receipts expire (default 300 s); a verifier must also accept each nonce only once, which needs storage on your side |
| Second person doing the actions | Partly | A second face in view fails the session after 0.5 s; someone fully out of frame is not detected |

## Known limitations

- **Blinks with glasses.** In the recorded glasses session, eye dips never separated from
  noise (`crates/liveness-core/tests/fixtures/README.md`). Prefer head-turn and mouth
  challenges, or give the user a choice.
- **Anti-spoofing scores are uncalibrated.** On clean digital renders of a real face (no print
  or screen) the score ranged 0.28-0.98 depending only on viewing angle. Calibrate before
  enforcing: record genuine and attack videos with your cameras and run
  `tools/calibrate_spoof.py`.
- **Frames are not bound to a physical camera.** The SHA-256 chain proves *which* frames were
  judged, not that they came from a live camera.
- **Fairness.** The face models' own evaluations are in their model cards (see
  `crates/liveness-core/models/README.md`); this project hasn't measured performance across
  demographics.

## Data handling

Frames are processed in memory and nothing is stored by the library. Receipts contain only the
outcome, counts, scores and a hash, no images or landmarks. If you keep footage for audits,
`frames_sha256` lets you prove which frames a receipt was based on.
