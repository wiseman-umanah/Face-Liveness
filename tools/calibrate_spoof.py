"""Picks a `min_real_score` from your own recordings.

Record a few videos of real people and a few attacks (a phone or laptop screen showing a face
video, a printed photo), run each through the Rust pipeline to get per-frame scores:

    ffmpeg -loglevel error -i genuine1.mp4 -f rawvideo -pix_fmt rgb24 - \
      | cargo run -p liveness-core --release --example video -- --size 640x480 --fps 30 --csv genuine1.csv

then compare them:

    python3 tools/calibrate_spoof.py --genuine genuine*.csv --attack screen*.csv print*.csv

Each recording is cut into windows as long as a typical liveness session; a window's score is
its mean real_score, which is exactly what `min_real_score` is compared with. The report uses
ISO/IEC 30107-3 terms: APCER = attacks wrongly accepted, BPCER = real people wrongly rejected.
"""

import argparse
import csv
import statistics


def windows(path, seconds):
    """Mean real_score of each `seconds`-long window of a recording."""
    rows = [r for r in csv.DictReader(open(path)) if r.get("real_score")]
    if not rows:
        raise SystemExit(f"{path}: no real_score column; record it with the `video` example's --csv")
    out, start, scores = [], int(rows[0]["ts_ms"]), []
    for r in rows:
        t = int(r["ts_ms"])
        if t - start >= seconds * 1000 and scores:
            out.append(statistics.mean(scores))
            start, scores = t, []
        scores.append(float(r["real_score"]))
    if scores:
        out.append(statistics.mean(scores))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--genuine", nargs="+", required=True, help="CSVs of real faces")
    parser.add_argument("--attack", nargs="+", required=True, help="CSVs of photos/screens")
    parser.add_argument("--window", type=float, default=5.0, help="session length in seconds (default 5)")
    parser.add_argument("--max-bpcer", type=float, default=0.05, help="real users you accept rejecting (default 5%%)")
    args = parser.parse_args()

    genuine = [w for p in args.genuine for w in windows(p, args.window)]
    attack = [w for p in args.attack for w in windows(p, args.window)]
    for name, xs in (("genuine", genuine), ("attack", attack)):
        q = statistics.quantiles(xs, n=20) if len(xs) > 1 else xs * 19
        print(f"{name:8} {len(xs):4} windows   mean {statistics.mean(xs):.3f}   p5 {q[0]:.3f}   median {statistics.median(xs):.3f}   p95 {q[-1]:.3f}")
    if len(genuine) < 20 or len(attack) < 20:
        print("\nfew windows: record more (and more varied) videos before trusting any threshold")

    print(f"\n{'threshold':>9}  {'APCER':>7}  {'BPCER':>7}")
    best = None
    for i in range(5, 100, 5):
        t = i / 100
        apcer = sum(x >= t for x in attack) / len(attack)
        bpcer = sum(x < t for x in genuine) / len(genuine)
        print(f"{t:9.2f}  {apcer:7.1%}  {bpcer:7.1%}")
        if bpcer <= args.max_bpcer and (best is None or apcer < best[1]):
            best = (t, apcer, bpcer)

    if best is None:
        print(f"\nno threshold keeps BPCER <= {args.max_bpcer:.0%}: the scores don't separate your data")
    else:
        t, apcer, bpcer = best
        print(
            f"\nsuggested min_real_score {t:.2f}: rejects {1 - apcer:.0%} of attack windows, "
            f"{bpcer:.0%} of genuine ones"
        )


if __name__ == "__main__":
    main()
