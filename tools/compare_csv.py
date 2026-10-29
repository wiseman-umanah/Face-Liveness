"""Compares two ts_ms,ear,mouth,yaw CSVs frame by frame (e.g. Rust vs MediaPipe).

    python3 tools/compare_csv.py rust.csv mediapipe.csv
"""

import csv
import statistics
import sys


def load(path):
    return {int(r["ts_ms"]): r for r in csv.DictReader(open(path))}


def main():
    a, b = load(sys.argv[1]), load(sys.argv[2])
    both = sorted(a.keys() & b.keys())
    print(f"frames with a face: {sys.argv[1]} {len(a)}, {sys.argv[2]} {len(b)}, both {len(both)}")
    if not both:
        return
    print(f"{'signal':<7}{'mean |diff|':>13}{'p95 |diff|':>12}{'max |diff|':>12}{'typical value':>15}")
    for col in ("ear", "mouth", "yaw"):
        diffs = sorted(abs(float(a[t][col]) - float(b[t][col])) for t in both)
        typical = statistics.median(float(b[t][col]) for t in both)
        p95 = diffs[int(0.95 * (len(diffs) - 1))]
        print(f"{col:<7}{statistics.mean(diffs):>13.4f}{p95:>12.4f}{diffs[-1]:>12.4f}{typical:>15.3f}")


if __name__ == "__main__":
    main()
