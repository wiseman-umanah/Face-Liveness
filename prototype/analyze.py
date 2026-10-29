import csv
import statistics
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "session.csv"
rows = list(csv.DictReader(open(path)))
if not rows:
    raise SystemExit("no rows: no face was detected")

ts = [int(r["ts_ms"]) for r in rows]
ear = [float(r["ear"]) for r in rows]
mouth = [float(r["mouth"]) for r in rows]
yaw = [float(r["yaw"]) for r in rows]


def pct(xs, ps=(1, 5, 25, 50, 75, 95, 99)):
    s = sorted(xs)
    return {p: round(s[min(len(s) - 1, int(p / 100 * len(s)))], 3) for p in ps}


dur = max((ts[-1] - ts[0]) / 1000, 0.001)
print(f"frames with a face: {len(rows)}  duration: {dur:.1f}s  ~{len(rows) / dur:.1f} fps")
for name, xs in (("ear", ear), ("mouth", mouth), ("yaw", yaw)):
    print(name, pct(xs))


def count_blinks(threshold, lo=50, hi=600):
    n, closed_since = 0, None
    for t, e in zip(ts, ear):
        if closed_since is None and e < threshold:
            closed_since = t
        elif closed_since is not None and e >= threshold:
            if lo <= t - closed_since <= hi:
                n += 1
            closed_since = None
    return n


med = statistics.median(ear)
print(f"\nmedian ear {med:.3f}  (roughly your open-eye value)")
print(f"fixed 0.20 threshold: {count_blinks(0.2)} blinks")
for frac in (0.5, 0.6, 0.7, 0.75, 0.8, 0.85):
    print(f"{frac:.2f} x median = {med * frac:.3f}: {count_blinks(med * frac)} blinks")
    