#!/usr/bin/env python3
"""The progress of the frontier campaign, read from its records.

    progress.py [--experiment DIR] [--round R]

Two tables. By diagonal: the instances of the grid, how many have every
run recorded, and the runs done over the runs owed. By configuration: the
runs complete, timed out, killed and failed, the median and the worst
seconds of the complete ones, and the agreement of the frontier with
the one-worker run of ours, point sets compared as exact rationals; for
Storm the agreement counts its default precision, so a smaller set is a
dropped point and not an error.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import CONFS, MODES  # noqa: E402

CONF_NAMES = [c for c, _ in CONFS]


def diagonal_of(key: str) -> int:
    n, i = key.split("-")[:2]
    return int(n) + int(i)


def grid_size(diag: int) -> int:
    pairs = sum(1 for n in range(1, 11) if 1 <= diag - n <= 10)
    return pairs * 10 * 9 * len(MODES)


def points_of(rec: dict, objectives: list[int] | None = None) -> frozenset | None:
    """The frontier of a record as a set of exact tuples, projected on the
    objectives Storm was asked, the components identically zero left out."""
    if rec.get("status") != "complete":
        return None
    if rec.get("tool") == "storm":
        return frozenset(tuple(p) for p in rec.get("points", []))
    pts = [p["value"] for p in rec.get("points", [])]
    if objectives is not None:
        pts = [[v[i] for i in objectives] for v in pts]
    return frozenset(tuple(v) for v in pts)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--experiment", type=Path, default=HERE / "experiment")
    p.add_argument("--round", type=int, default=1)
    a = p.parse_args()
    results = a.experiment / "results"
    if not results.exists():
        print(f"no records under {results}")
        return 1

    by_diag: dict[int, dict] = defaultdict(lambda: {"inst": 0, "done": 0, "runs": 0})
    by_conf: dict[str, dict] = {c: {"complete": 0, "timeout": 0, "killed": 0, "failed": 0,
                                    "secs": [], "agree": 0, "compared": 0, "storm_smaller": 0}
                                for c in CONF_NAMES}
    latest = (0.0, "")
    for inst in sorted(results.iterdir()):
        recs = {}
        for f in inst.glob(f"*.r{a.round}.json"):
            recs[f.name.split(".")[0]] = json.loads(f.read_text())
            latest = max(latest, (f.stat().st_mtime, inst.name))
        if not recs:
            continue
        d = by_diag[diagonal_of(inst.name)]
        d["inst"] += 1
        d["runs"] += len(recs)
        if len(recs) == len(CONF_NAMES):
            d["done"] += 1
        ours = points_of(recs["w1"]) if "w1" in recs else None
        for c, r in recs.items():
            s = by_conf[c]
            st = r.get("status", "failed")
            s[st if st in s else "failed"] += 1
            if st == "complete":
                s["secs"].append(r.get("seconds", 0.0))
                if c != "w1" and ours is not None:
                    if c == "storm":
                        pts = points_of(r)
                        ours_here = points_of(recs["w1"], r.get("objectives"))
                    else:
                        pts, ours_here = points_of(r), ours
                    s["compared"] += 1
                    if pts == ours_here:
                        s["agree"] += 1
                    elif c == "storm" and pts is not None and pts < ours_here:
                        s["storm_smaller"] += 1

    total_runs = sum(d["runs"] for d in by_diag.values())
    total_done = sum(d["done"] for d in by_diag.values())
    log = a.experiment / "campaign.log"
    started = log.stat().st_ctime if log.exists() else time.time()
    hours = (time.time() - started) / 3600
    print(f"round {a.round}: {total_done} instances complete, {total_runs} runs recorded, "
          f"{hours:.1f} h since the log began, last record {latest[1]}")
    print()
    print(f"{'diagonal':>8} {'grid':>6} {'started':>8} {'complete':>9} {'runs':>12}")
    for diag in sorted(by_diag):
        d = by_diag[diag]
        owed = d["inst"] * len(CONF_NAMES)
        print(f"{diag:>8} {grid_size(diag):>6} {d['inst']:>8} {d['done']:>9} {d['runs']:>5}/{owed:<6}")
    print()
    print(f"{'conf':>8} {'complete':>9} {'timeout':>8} {'killed':>7} {'failed':>7} "
          f"{'median s':>9} {'worst s':>9} {'agree w1':>10}")
    for c in CONF_NAMES:
        s = by_conf[c]
        med = statistics.median(s["secs"]) if s["secs"] else 0.0
        worst = max(s["secs"]) if s["secs"] else 0.0
        agree = "" if c == "w1" else f"{s['agree']}/{s['compared']}"
        if c == "storm" and s["storm_smaller"]:
            agree += f" ({s['storm_smaller']} fewer)"
        print(f"{c:>8} {s['complete']:>9} {s['timeout']:>8} {s['killed']:>7} {s['failed']:>7} "
              f"{med:>9.3f} {worst:>9.3f} {agree:>10}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
