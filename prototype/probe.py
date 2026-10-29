import argparse
import csv
import math
import os
import time

import cv2
import mediapipe as mp

# MediaPipe face-mesh landmark indices.
# Order per eye: p1 corner, p2 upper, p3 upper, p4 corner, p5 lower, p6 lower
RIGHT_EYE = [33, 160, 158, 133, 153, 144]
LEFT_EYE = [362, 385, 387, 263, 373, 380]
MOUTH_TOP, MOUTH_BOTTOM, MOUTH_L, MOUTH_R = 13, 14, 78, 308
NOSE, FACE_L, FACE_R = 1, 234, 454

EAR_THRESHOLD = 0.2          # placeholder: tune with your own data
BLINK_MIN_MS, BLINK_MAX_MS = 50, 600


def pt(lm, i, w, h):
    # Landmarks are normalized 0..1; convert to pixels so the image
    # aspect ratio doesn't distort distances.
    return (lm[i].x * w, lm[i].y * h)


def dist(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def ear(lm, idx, w, h):
    p = [pt(lm, i, w, h) for i in idx]
    horizontal = dist(p[0], p[3])
    return (dist(p[1], p[5]) + dist(p[2], p[4])) / (2 * horizontal) if horizontal else 0.0


def mouth_ratio(lm, w, h):
    horizontal = dist(pt(lm, MOUTH_L, w, h), pt(lm, MOUTH_R, w, h))
    vertical = dist(pt(lm, MOUTH_TOP, w, h), pt(lm, MOUTH_BOTTOM, w, h))
    return vertical / horizontal if horizontal else 0.0


def yaw_ratio(lm, w, h):
    # ~0.5 facing the camera; moves toward 0 or 1 as the head turns.
    left, right, nose = lm[FACE_L].x * w, lm[FACE_R].x * w, lm[NOSE].x * w
    return (nose - left) / (right - left) if right != left else 0.5


class BlinkDetector:
    def __init__(self):
        self.closed_since = None

    def update(self, ear_value, now_ms):
        closed = ear_value < EAR_THRESHOLD
        if self.closed_since is None and closed:
            self.closed_since = now_ms
        elif self.closed_since is not None and not closed:
            duration = now_ms - self.closed_since
            self.closed_since = None
            return BLINK_MIN_MS <= duration <= BLINK_MAX_MS
        return False


def main():
    parser = argparse.ArgumentParser(description="Record per-frame face measurements to CSV.")
    parser.add_argument("out", nargs="?", default="session.csv", help="output CSV (default: session.csv)")
    parser.add_argument("--camera", type=int, default=0, help="camera index (default: 0)")
    parser.add_argument("--force", action="store_true", help="overwrite the output file if it exists")
    args = parser.parse_args()

    if os.path.exists(args.out) and not args.force:
        raise SystemExit(f"{args.out} already exists; pick another name or pass --force")

    cap = cv2.VideoCapture(args.camera)
    if not cap.isOpened():
        raise SystemExit(f"could not open camera {args.camera}")

    face_mesh = mp.solutions.face_mesh.FaceMesh(max_num_faces=1)
    blink = BlinkDetector()
    blinks, t0 = 0, time.monotonic()

    with open(args.out, "w", newline="") as f:
        out = csv.writer(f)
        out.writerow(["ts_ms", "ear", "mouth", "yaw"])

        while True:
            ok, frame = cap.read()
            if not ok:
                print("failed to read a frame")
                break
            h, w = frame.shape[:2]
            ts = int((time.monotonic() - t0) * 1000)

            rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            result = face_mesh.process(rgb)

            text = "no face"
            if result.multi_face_landmarks:
                lm = result.multi_face_landmarks[0].landmark
                e = (ear(lm, LEFT_EYE, w, h) + ear(lm, RIGHT_EYE, w, h)) / 2
                m = mouth_ratio(lm, w, h)
                y = yaw_ratio(lm, w, h)
                if blink.update(e, ts):
                    blinks += 1
                out.writerow([ts, f"{e:.3f}", f"{m:.3f}", f"{y:.3f}"])
                text = f"EAR {e:.2f}  mouth {m:.2f}  yaw {y:.2f}  blinks {blinks}"

            print(text + "   ", end="\r", flush=True)
            cv2.putText(frame, text, (10, 30), cv2.FONT_HERSHEY_SIMPLEX,
                        0.7, (0, 255, 0), 2)
            cv2.imshow("probe", frame)
            if cv2.waitKey(1) & 0xFF == 27:  # Esc to quit
                break

    face_mesh.close()
    cap.release()
    cv2.destroyAllWindows()


if __name__ == "__main__":
    main()
    