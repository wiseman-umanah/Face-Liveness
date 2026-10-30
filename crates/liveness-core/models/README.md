# Bundled models

## Face detection and landmarks

These two models come from Google's [MediaPipe Face Landmarker](https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker)
bundle, converted from TFLite to ONNX so they run on [tract](https://github.com/sonos/tract)
(pure Rust, so the same code builds for servers, WASM and mobile). They are the same
models `prototype/probe.py` uses through MediaPipe.

| File | Model | Input | Outputs |
|---|---|---|---|
| `face_detector.onnx` | BlazeFace short-range | 1x128x128x3, RGB in [-1, 1] | 896 anchor boxes + 6 keypoints, 896 scores |
| `face_landmarks_detector.onnx` | Face mesh v2 | 1x256x256x3, RGB in [0, 1] | 478 landmarks (x, y, z), face-presence logit |

**License:** Apache-2.0 (MediaPipe models), same as this project. See the model cards for
intended use and fairness evaluation:
[BlazeFace short-range](https://storage.googleapis.com/mediapipe-assets/MediaPipe%20BlazeFace%20Model%20Card%20(Short%20Range).pdf),
[Face Mesh V2](https://storage.googleapis.com/mediapipe-assets/Model%20Card%20MediaPipe%20Face%20Mesh%20V2.pdf).

**Source:** `face_landmarker.task`, float16 version 1:
<https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task>
(sha256 `64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff`).

**Conversion:** `./convert.sh` (tf2onnx 1.17.0, TensorFlow 2.21.0, ONNX opset 17). The FP16
weights are stored as FP32 in ONNX. Re-running it gives identical graphs and weights; the file
hashes can still differ because tf2onnx records the source file path in the metadata.

**Accuracy after conversion:** on the same random input, tract's outputs match the original
TFLite models to within 1e-4 (detector scores and boxes) and 3e-4 px (landmarks).

| File | sha256 (as committed) |
|---|---|
| `face_detector.onnx` | `262dbc004ee1313ee7e76678ee6fec030d7b5bbf0ea44fe7b93740e48d3c08d2` |
| `face_landmarks_detector.onnx` | `bbabe43f2d3fae0e1a30937af8dd5175ae90f11c7734ca5d60940dbf0861e1db` |

## Passive anti-spoofing

| File | Model | Input | Output |
|---|---|---|---|
| `antispoof_minifasnet_v2_2.7.onnx` | MiniFASNetV2, trained on face crops scaled 2.7x | 1x3x80x80, BGR, 0-255 | 3 logits (class 1 = real face) |
| `antispoof_minifasnet_v1se_4.0.onnx` | MiniFASNetV1SE, trained on face crops scaled 4.0x | 1x3x80x80, BGR, 0-255 | 3 logits (class 1 = real face) |

**Source:** minivision-ai/[Silent-Face-Anti-Spoofing](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing),
commit `b6d5f04ad78778917853b25c778acef6d5626d15`, files `resources/anti_spoof_models/*.pth`.
**License:** Apache-2.0.

**Conversion:** `./convert_antispoof.sh` (PyTorch 2.15.0 CPU, ONNX opset 17). It runs only the
repository's model definitions (`src/model_lib/MiniFASNet.py`, plain PyTorch layers) and loads
the weights with `weights_only=True`, so the pickled checkpoints can't execute code.

**Accuracy after conversion:** on the astronaut test photo, the Rust pipeline (tract + ported
crop) scores 0.8536 and minivision's own crop code + PyTorch 0.8553.

**Calibration:** the models were trained on minivision's data, not your cameras. On clean renders
of a real face the score varied 0.28-0.98 with viewing angle alone, so the library only reports
it until you set `min_real_score`; pick that value with `tools/calibrate_spoof.py` on your own
genuine and attack recordings. See `docs/threat-model.md`.

| File | sha256 (as committed) |
|---|---|
| `antispoof_minifasnet_v2_2.7.onnx` | `3a193e9764077a5114e972a1a9c808375885aa4de3f29edb14af161007d3a816` |
| `antispoof_minifasnet_v1se_4.0.onnx` | `f82ef6dc973d3675a645bd4e026e72c546e4069e37bed7123ad89390522d09fd` |
