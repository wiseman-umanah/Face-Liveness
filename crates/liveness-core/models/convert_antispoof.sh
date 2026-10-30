#!/usr/bin/env bash
# Regenerates the passive anti-spoofing models (MiniFASNet) as ONNX from minivision's
# Silent-Face-Anti-Spoofing release (Apache-2.0). Needs git and uv; uses a throwaway
# Python 3.12 environment with CPU-only PyTorch (not under /tmp: it is ~1 GB).
set -euo pipefail

REPO="https://github.com/minivision-ai/Silent-Face-Anti-Spoofing.git"
COMMIT="b6d5f04ad78778917853b25c778acef6d5626d15"

out_dir="$(cd "$(dirname "$0")" && pwd)"
work="${XDG_CACHE_HOME:-$HOME/.cache}/face-liveness-convert-antispoof"
mkdir -p "$work"

if [ ! -d "$work/repo" ]; then
  git clone -q "$REPO" "$work/repo"
fi
git -C "$work/repo" checkout -q "$COMMIT"

[ -d "$work/.venv" ] || uv venv -q --python 3.12 "$work/.venv"
VIRTUAL_ENV="$work/.venv" uv pip install -q --index-url https://download.pytorch.org/whl/cpu "torch==2.15.0"
VIRTUAL_ENV="$work/.venv" uv pip install -q "onnx==1.23.2" "onnxscript"

"$work/.venv/bin/python" -I - "$work/repo" "$out_dir" <<'PY'
import sys
from pathlib import Path

import torch

repo, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
sys.path.insert(0, str(repo))
from src.model_lib.MiniFASNet import MiniFASNetV1SE, MiniFASNetV2  # model definitions only

# name in this repo, weights file, constructor; 80x80 input -> conv6 kernel (5, 5)
models = [
    ("antispoof_minifasnet_v2_2.7.onnx", "2.7_80x80_MiniFASNetV2.pth", MiniFASNetV2),
    ("antispoof_minifasnet_v1se_4.0.onnx", "4_0_0_80x80_MiniFASNetV1SE.pth", MiniFASNetV1SE),
]
for out_name, weights, build in models:
    model = build(conv6_kernel=(5, 5))
    state = torch.load(repo / "resources/anti_spoof_models" / weights, map_location="cpu", weights_only=True)
    model.load_state_dict({k.removeprefix("module."): v for k, v in state.items()})
    model.eval()
    torch.onnx.export(
        model,
        torch.zeros(1, 3, 80, 80),
        out_dir / out_name,
        input_names=["input"],
        output_names=["logits"],
        opset_version=17,
        dynamo=False,
    )
    print("wrote", out_dir / out_name)
PY
sha256sum "$out_dir"/antispoof_*.onnx
