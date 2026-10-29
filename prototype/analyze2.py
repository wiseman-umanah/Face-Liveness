import csv
import sys
from collections import deque
from statistics import median

rows = list(csv.DictReader(open(sys.argv[1])))
ts = [int(r["ts_ms"]) for r in rows]
ear = [float(r["ear"]) for r in rows]


def count_blinks(enter, depth, gap=0.1, max_ms=600):
    # enter: closure starts below this fraction of baseline
    # gap:   must recover above (enter + gap) x baseline (hysteresis)
    # depth: the closure must dip below this fraction of baseline
    hist = deque(maxlen=150)  # ~8 s of frames
    n, start, lowest = 0, None, 1.0
    for t, e in zip(ts, ear):
        hist.append(e)
        if len(hist) < 30:
            continue
        r = e / median(hist)
        if start is None:
            if r < enter:
                start, lowest = t, r
        else:
            lowest = min(lowest, r)
            if r >= enter + gap:
                if t - start <= max_ms and lowest < depth:
                    n += 1
                start = None
    return n


print("enter  depth  blinks")
for enter in (0.7, 0.8, 0.85):
    for depth in (0.5, 0.6, 0.7):
        if depth < enter:
            print(f"{enter:.2f}   {depth:.2f}   {count_blinks(enter, depth)}")
            