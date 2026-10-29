# Bundled models

Both models come from Google's [MediaPipe Face Landmarker](https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker)
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
