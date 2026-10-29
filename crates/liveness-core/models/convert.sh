#!/usr/bin/env bash
# Regenerates the bundled ONNX models from Google's MediaPipe Face Landmarker bundle.
# Needs uv; uses a throwaway Python 3.12 environment (TensorFlow is ~2 GB, so not under /tmp).
set -euo pipefail

TASK_URL="https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task"
TASK_SHA256="64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff"

out_dir="$(cd "$(dirname "$0")" && pwd)"
work="${XDG_CACHE_HOME:-$HOME/.cache}/face-liveness-convert"
mkdir -p "$work"

curl -sSfL -o "$work/face_landmarker.task" "$TASK_URL"
echo "$TASK_SHA256  $work/face_landmarker.task" | sha256sum -c -

python3 -c "import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2], ['face_detector.tflite', 'face_landmarks_detector.tflite'])" \
  "$work/face_landmarker.task" "$work"

[ -d "$work/.venv" ] || uv venv -q --python 3.12 "$work/.venv"
VIRTUAL_ENV="$work/.venv" uv pip install -q "tf2onnx==1.17.0" "tensorflow==2.21.0"

for model in face_detector face_landmarks_detector; do
  "$work/.venv/bin/python" -I -m tf2onnx.convert --opset 17 \
    --tflite "$work/$model.tflite" --output "$out_dir/$model.onnx"
done
sha256sum "$out_dir"/*.onnx
