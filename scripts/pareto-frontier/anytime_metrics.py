#!/usr/bin/env python3
"""Per-point anytime metrics, from the buffers the campaign already holds.

    anytime_metrics.py                        everything, into the paper data folder
    anytime_metrics.py --limit 50 --out /tmp/x.json      a taste

One pass over the one-phase records (`experiment/results/<instance>/
onephase-<ablation>-w<workers>.r1.json`) and their anytime buffers
(`...buffer.jsonl.gz`, one line per apex with the second it entered the
record), writing one JSON of aggregates. The analysis of `analysis.py` reads
the sampled `curve` of the truncated runs alone; this one reads the buffers
themselves, of the completed runs too, for four quantities the curves do not
carry:

  * `first_point`: the second at which the first apex of a run entered the
    record, by configuration and status, quartiles;
  * `tail`: on the completed runs, the certification tail, the time between
    the instant the last surviving point was found and the end of the search,
    in seconds and as a fraction of the run; the search past that instant
    only certifies, it finds nothing new;
  * `point_delay`: on the completed runs, for every point of the final
    frontier, the time between its discovery and the end of the search that
    certifies it, pooled into histograms (log-spaced seconds, and the
    fraction of the run) ready for a CDF;
  * `precision`: on every instance that one configuration cut and another
    completed, how many of the candidates of the cut run are points of the
    exact frontier; the remainder is improvable, and a candidate below the
    frontier would break soundness, so those are counted apart and must be
    zero.

Conventions as in `analysis.py`: the configuration is the file name, not the
field inside it; the two-phase records in the same folders are not read; the
grid is the diagonals 2 to 7 of the paper; a run is complete when the search
closed by itself within the budget. The discovery time of a point is the
first buffer line carrying its exact value, the rational strings compared as
written. The builds seen in the records (`host.sdcpi_commit`, `host.storm`)
are tallied for the appendix.
"""

from __future__ import annotations

import argparse
import collections
import gzip
import json
import math
import os
import re
import statistics
import sys
from multiprocessing import Pool
from pathlib import Path

HERE = Path(__file__).resolve().parent
RESULTS = HERE / "experiment" / "results"
PAPER_DATA = Path("/Users/pietrosala/Documents/github/cpi-semantics-mdp/parallel-paper/data")

CONF_RE = re.compile(r"^onephase-(?P<ablation>[a-z]+)-w(?P<workers>\d+)\.r\d+\.json$")
FOLDER_RE = re.compile(r"^(\d+)-(\d+)-(\d+)-(\d+)-(.+)$")
DIAGONALS = range(2, 8)
BASELINE_ORDER = ["onephase-both-w1", "onephase-both-w2", "onephase-both-w4", "onephase-both-w8"]

# log-spaced bins for the delay in seconds, 1e-4 s to 126 s, 40 bins a decade
ABS_LO, ABS_STEP, ABS_BINS = -4.0, 0.025, 246
# linear bins for the delay as a fraction of the run, 0 to 1, half a percent each
FRAC_BINS = 200


def abs_bin(seconds: float) -> int:
    if seconds <= 0.0:
        return 0
    return min(ABS_BINS - 1, max(0, int((math.log10(seconds) - ABS_LO) / ABS_STEP)))


def frac_bin(fraction: float) -> int:
    return min(FRAC_BINS - 1, max(0, int(fraction * FRAC_BINS)))


def scan_buffer_complete(path: Path, needed: set[tuple[str, ...]]):
    """First apex second, end second, and the first second of every needed
    value; the stream stops once every needed value has been seen and the end
    line read."""
    first_t = None
    end_t = None
    found: dict[tuple[str, ...], float] = {}
    with gzip.open(path, "rt") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if "n" in e:
                if first_t is None:
                    first_t = e["t"]
                if len(found) < len(needed):
                    v = tuple(e["v"])
                    if v in needed and v not in found:
                        found[v] = e["t"]
            elif e.get("end"):
                end_t = e["t"]
                if len(found) == len(needed):
                    break
    return first_t, end_t, found


def scan_buffer_first(path: Path):
    """The first apex second alone, for the truncated runs."""
    with gzip.open(path, "rt") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if "n" in e:
                return e["t"]
            if e.get("end"):
                return None
    return None


def dominated_or_equal(p: tuple, c: tuple) -> bool:
    """Whether the exact point `p` is at or below the candidate `c` on every
    component, in rational arithmetic."""
    from fractions import Fraction
    return all(Fraction(a) <= Fraction(b) for a, b in zip(p, c))


def scan_instance(folder: str):
    """One instance folder: the scalars of every one-phase run, the pooled
    histogram increments, the precision of the candidates of the cut runs,
    and the builds seen."""
    out = {
        "scalars": [],          # (conf, diag, status, first_t, end_t, search_s, tail, points_n, unmatched)
        "abs_hist": collections.Counter(),    # (conf, bin) -> n
        "tail_hist": collections.Counter(),   # (conf, bin) -> n, tail as a fraction of the run
        "frac_hist": collections.Counter(),   # (conf, bin) -> n
        "delay_sum": collections.Counter(),   # conf -> seconds summed, and conf+"/n" -> points
        "precision": [],        # (conf, diag, candidates, on_frontier, unsound)
        "frontier_mismatch": 0,
        "builds": collections.Counter(),
        "missing_buffer": 0,
    }
    name = os.path.basename(folder)
    m = FOLDER_RE.match(name)
    if not m:
        return None
    diag = int(m.group(1)) + int(m.group(2))
    if diag not in DIAGONALS:
        return None
    records: dict[str, dict] = {}
    for f in sorted(os.listdir(folder)):
        if CONF_RE.match(f):
            try:
                with open(os.path.join(folder, f)) as fh:
                    records[f[: f.index(".r")]] = (f, json.load(fh))
            except (OSError, json.JSONDecodeError, ValueError):
                continue
    complete_fronts: dict[str, frozenset] = {}
    for conf, (fname, rec) in records.items():
        host = rec.get("host") or {}
        out["builds"][("sdcpi", host.get("sdcpi_commit"))] += 1
        out["builds"][("storm", host.get("storm"))] += 1
        status = rec.get("status")
        points = [tuple(p["value"]) for p in rec.get("points") or []]
        buf = Path(folder) / (fname[: -len(".json")] + ".buffer.jsonl.gz")
        if status == "complete":
            complete_fronts[conf] = frozenset(points)
            if not buf.exists():
                out["missing_buffer"] += 1
                continue
            first_t, end_t, found = scan_buffer_complete(buf, set(points))
            if end_t is None:
                end_t = rec.get("end_seconds")
            if first_t is None or end_t is None:
                continue
            unmatched = len(points) - len(found)
            tail = max(0.0, end_t - max(found.values())) if found else None
            if tail is not None and end_t > 0:
                out["tail_hist"][(conf, frac_bin(tail / end_t))] += 1
            for t in found.values():
                delay = max(0.0, end_t - t)
                out["abs_hist"][(conf, abs_bin(delay))] += 1
                out["frac_hist"][(conf, frac_bin(delay / end_t if end_t > 0 else 0.0))] += 1
                out["delay_sum"][conf] += delay
                out["delay_sum"][conf + "/n"] += 1
            out["scalars"].append((conf, diag, "complete", first_t, end_t,
                                   rec.get("search_seconds"), tail, len(points), unmatched))
        elif status == "truncated":
            first_t = scan_buffer_first(buf) if buf.exists() else None
            if not buf.exists():
                out["missing_buffer"] += 1
            out["scalars"].append((conf, diag, "truncated", first_t, None,
                                   None, None, len(points), 0))
    # the exact frontier of the instance, for the candidates of the cut runs
    ref = None
    for conf in BASELINE_ORDER:
        if conf in complete_fronts:
            ref = complete_fronts[conf]
            break
    if ref is None and complete_fronts:
        ref = next(iter(complete_fronts.values()))
    if ref is not None:
        if any(f != ref for f in complete_fronts.values()):
            out["frontier_mismatch"] += 1
        for conf, (fname, rec) in records.items():
            if rec.get("status") != "truncated":
                continue
            cands = [tuple(p["value"]) for p in rec.get("points") or []]
            on = sum(1 for c in cands if c in ref)
            unsound = 0
            for c in cands:
                if c in ref:
                    continue
                if not any(dominated_or_equal(p, c) for p in ref):
                    unsound += 1
            out["precision"].append((conf, diag, len(cands), on, unsound))
    return out


def quartiles(xs: list[float]) -> dict | None:
    xs = [x for x in xs if x is not None]
    if len(xs) < 2:
        return {"n": len(xs), "median": xs[0]} if xs else None
    q = statistics.quantiles(xs, n=4)
    return {"n": len(xs), "q1": round(q[0], 6), "median": round(q[1], 6),
            "q3": round(q[2], 6), "p90": round(statistics.quantiles(xs, n=10)[8], 6),
            "max": round(max(xs), 6)}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--results", type=Path, default=RESULTS)
    ap.add_argument("--out", type=Path, default=PAPER_DATA / "anytime-metrics.json")
    ap.add_argument("--procs", type=int, default=8)
    ap.add_argument("--limit", type=int, default=None, help="instances, for a taste")
    a = ap.parse_args()

    folders = sorted(str(f.path) for f in os.scandir(a.results) if f.is_dir())
    if a.limit:
        folders = folders[: a.limit]
    scalars, precision = [], []
    abs_hist = collections.Counter()
    tail_hist = collections.Counter()
    frac_hist = collections.Counter()
    delay_sum = collections.Counter()
    builds = collections.Counter()
    mismatches, missing = 0, 0
    done = 0
    with Pool(a.procs) as pool:
        for r in pool.imap_unordered(scan_instance, folders, chunksize=16):
            done += 1
            if done % 500 == 0:
                print(f"{done}/{len(folders)} folders", file=sys.stderr, flush=True)
            if r is None:
                continue
            scalars.extend(r["scalars"])
            precision.extend(r["precision"])
            abs_hist.update(r["abs_hist"])
            tail_hist.update(r["tail_hist"])
            frac_hist.update(r["frac_hist"])
            delay_sum.update(r["delay_sum"])
            builds.update(r["builds"])
            mismatches += r["frontier_mismatch"]
            missing += r["missing_buffer"]

    by_conf: dict[str, dict] = {}
    confs = sorted({s[0] for s in scalars})
    for conf in confs:
        rows = [s for s in scalars if s[0] == conf]
        comp = [s for s in rows if s[2] == "complete"]
        trunc = [s for s in rows if s[2] == "truncated"]
        d = {
            "complete": len(comp),
            "truncated": len(trunc),
            "unmatched_points": sum(s[8] for s in comp),
            "first_point_complete": quartiles([s[3] for s in comp]),
            "first_point_truncated": quartiles([s[3] for s in trunc]),
            "tail_seconds": quartiles([s[6] for s in comp]),
            "tail_fraction": quartiles([s[6] / s[4] for s in comp
                                        if s[6] is not None and s[4]]),
            "points_delay_mean": (round(delay_sum[conf] / delay_sum[conf + "/n"], 6)
                                   if delay_sum[conf + "/n"] else None),
            "points_counted": delay_sum[conf + "/n"],
            "delay_hist_abs": [abs_hist[(conf, b)] for b in range(ABS_BINS)],
            "delay_hist_frac": [frac_hist[(conf, b)] for b in range(FRAC_BINS)],
            "tail_hist_frac": [tail_hist[(conf, b)] for b in range(FRAC_BINS)],
        }
        by_conf[conf] = d

    prec: dict[str, dict] = {}
    for conf in sorted({p[0] for p in precision}):
        rows = [p for p in precision if p[0] == conf]
        cands = sum(p[2] for p in rows)
        on = sum(p[3] for p in rows)
        prec[conf] = {
            "runs": len(rows),
            "candidates": cands,
            "on_frontier": on,
            "rate": round(on / cands, 4) if cands else None,
            "unsound": sum(p[4] for p in rows),
            "rate_by_run": quartiles([p[3] / p[2] for p in rows if p[2]]),
        }
    prec_diag: dict[str, dict] = {}
    for diag in sorted({p[1] for p in precision}):
        rows = [p for p in precision if p[1] == diag]
        cands = sum(p[2] for p in rows)
        prec_diag[str(diag)] = {
            "runs": len(rows), "candidates": cands,
            "on_frontier": sum(p[3] for p in rows),
            "rate": round(sum(p[3] for p in rows) / cands, 4) if cands else None,
        }

    first_by_diag: dict[str, dict] = {}
    for diag in DIAGONALS:
        rows = [s for s in scalars if s[1] == diag]
        if rows:
            first_by_diag[str(diag)] = {
                "complete": quartiles([s[3] for s in rows if s[2] == "complete"]),
                "truncated": quartiles([s[3] for s in rows if s[2] == "truncated"]),
            }

    out = {
        "source": str(a.results),
        "diagonals": [d for d in DIAGONALS],
        "runs": len(scalars),
        "complete": sum(1 for s in scalars if s[2] == "complete"),
        "truncated": sum(1 for s in scalars if s[2] == "truncated"),
        "missing_buffers": missing,
        "frontier_mismatch_instances": mismatches,
        "unmatched_points_total": sum(s[8] for s in scalars if s[2] == "complete"),
        "builds": {f"{kind}:{val}": n for (kind, val), n in sorted(builds.items())},
        "hist_bins": {"abs_log10_lo": ABS_LO, "abs_log10_step": ABS_STEP,
                       "abs_bins": ABS_BINS, "frac_bins": FRAC_BINS},
        "by_conf": by_conf,
        "first_point_by_diagonal": first_by_diag,
        "precision_by_conf": prec,
        "precision_by_diagonal": prec_diag,
    }
    a.out.parent.mkdir(parents=True, exist_ok=True)
    with open(a.out, "w") as fh:
        json.dump(out, fh, indent=1)
    print(f"{a.out}: {len(scalars)} runs, {out['complete']} complete, "
          f"{out['truncated']} truncated, {missing} buffers missing, "
          f"{mismatches} frontier mismatches, "
          f"{out['unmatched_points_total']} unmatched points", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
