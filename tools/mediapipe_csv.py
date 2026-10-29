"""Runs MediaPipe Face Landmarker on a video and writes ts_ms,ear,mouth,yaw per frame with a
face: the same CSV the Rust `video` example writes, so the two pipelines can be compared.

    python tools/mediapipe_csv.py clip.mp4 out.csv --model face_landmarker.task [--fps 30]

Needs `mediapipe` and `opencv-python` (e.g. the prototype/.venv environment). Timestamps are
frame index / fps, matching `cargo run --example video -- --fps N`.
"""

import argparse
import csv
import math

import cv2
import mediapipe as mp
from mediapipe.tasks.python import BaseOptions, vision

# Same indices as prototype/probe.py and the Rust pipeline.
RIGHT_EYE = [33, 160, 158, 133, 153, 144]
LEFT_EYE = [362, 385, 387, 263, 373, 380]


def dist(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def ear(p, idx):
    q = [p[i] for i in idx]
    horizontal = dist(q[0], q[3])
    return (dist(q[1], q[5]) + dist(q[2], q[4])) / (2 * horizontal) if horizontal else 0.0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("video")
    parser.add_argument("out")
    parser.add_argument("--model", required=True, help="face_landmarker.task")
    parser.add_argument("--fps", type=float, default=30.0)
    args = parser.parse_args()

    options = vision.FaceLandmarkerOptions(
        base_options=BaseOptions(model_asset_path=args.model),
        running_mode=vision.RunningMode.VIDEO,
    )
    landmarker = vision.FaceLandmarker.create_from_options(options)
    cap = cv2.VideoCapture(args.video)
    frames = faces = 0
    with open(args.out, "w", newline="") as f:
        out = csv.writer(f)
        out.writerow(["ts_ms", "ear", "mouth", "yaw"])
        while True:
            ok, bgr = cap.read()
            if not ok:
                break
            ts = int(frames * 1000 / args.fps)
            frames += 1
            h, w = bgr.shape[:2]
            image = mp.Image(image_format=mp.ImageFormat.SRGB, data=cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB))
            result = landmarker.detect_for_video(image, ts)
            if not result.face_landmarks:
                continue
            faces += 1
            p = [(lm.x * w, lm.y * h) for lm in result.face_landmarks[0]]
            e = (ear(p, LEFT_EYE) + ear(p, RIGHT_EYE)) / 2
            width = dist(p[78], p[308])
            m = dist(p[13], p[14]) / width if width else 0.0
            span = p[454][0] - p[234][0]
            y = min(max((p[1][0] - p[234][0]) / span, 0.0), 1.0) if span else 0.5
            out.writerow([ts, f"{e:.3f}", f"{m:.3f}", f"{y:.3f}"])
    print(f"{frames} frames, face in {faces}")


if __name__ == "__main__":
    main()
