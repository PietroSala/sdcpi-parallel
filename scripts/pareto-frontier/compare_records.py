#!/usr/bin/env python3
"""A rerun of the campaign set against its records, run by run.

    compare_records.py --rerun <experiment folder of run.py> \\
        --results <records> --instances <records> --models <records>

The rerun is the folder that `run.py --experiment` wrote on a slice of the
grid, with its `instances/`, `models/` and `results/`; the three other
folders are the same slice extracted from the two archives of the campaign,
the records of Storm merged into the results of the tool. For every
instance of the rerun:

  * the instance the generator wrote must be the one of the records, byte
    for byte, and so must the PRISM model and the query that Storm read;
  * a run complete in the rerun and in the records must give the same
    frontier, the same set of vectors read as exact rationals, for every
    configuration of ours and for Storm alike;
  * every other pair of statuses is counted and not judged: the budget cuts
    where the speed of the machine puts it, and a run that closed a little
    under the budget in the campaign may be cut here, or the reverse.

Then the comparison with Storm and the ablation study of the paper on the
slice, computed by `analysis.py` from the rerun and from the records, side
by side. The last line is `RERUN AGREES WITH THE RECORDS`, and the exit
status is not zero when an instance, a model or a frontier differs, or when
a run of the slice has no record in the rerun.
"""

from __future__ import annotations

import argparse
import collections
import json
import sys
from fractions import Fraction
from pathlib import Path

import analysis

RECORD_SUFFIX = analysis.RECORD_SUFFIX
STORM = analysis.STORM_CONF
COMPLETE = analysis.COMPLETE


def frontier(conf: str, rec: dict) -> frozenset:
    """The points of a run as a set of exact vectors: a point of ours is a
    dictionary carrying its value, a point of Storm is the vector itself."""
    if conf == STORM:
        return frozenset(tuple(Fraction(str(x)) for x in p) for p in rec.get("points") or [])
    return frozenset(tuple(Fraction(str(x)) for x in p["value"]) for p in rec.get("points") or [])


def same_bytes(a: Path, b: Path) -> bool | None:
    """Whether two files are the same, and nothing when either is missing."""
    if not a.exists() or not b.exists():
        return None
    return a.read_bytes() == b.read_bytes()


def query_of(storm: dict | None) -> str | None:
    """The query a record of Storm was run on, read off its command: the
    earliest models of the archive have no query file beside them, and every
    record carries the command. Nothing when Storm was not called, every
    impact being zero."""
    command = (storm or {}).get("command") or []
    return command[command.index("--prop") + 1] if "--prop" in command else None


def compare_files(rerun: Path, instances: Path, models: Path,
                  records: Path) -> tuple[dict, list[str]]:
    """The instances, the models and the queries of the rerun against those
    of the records."""
    counts = collections.Counter()
    bad = []
    for mine in sorted((rerun / "instances").rglob("*.yaml")):
        rel = mine.relative_to(rerun / "instances")
        same = same_bytes(mine, instances / rel)
        counts["instances"] += 1
        counts["instances_same" if same else "instances_differ"] += 1
        if not same:
            bad.append(f"instance {rel}: " + ("not in the records" if same is None else "differs"))
    for mine in sorted((rerun / "models").glob("*.prism")):
        counts["models"] += 1
        same = same_bytes(mine, models / mine.name)
        counts["models_same" if same else "models_differ"] += 1
        if not same:
            bad.append(f"model {mine.stem}: " + ("not in the records" if same is None
                                                 else "differs from the one Storm read"))
        theirs = query_of(analysis.load(str(records / mine.stem / (STORM + RECORD_SUFFIX))))
        if theirs is None:
            continue
        ours = mine.with_suffix(".prop")
        counts["queries"] += 1
        if ours.exists() and ours.read_text().strip() == theirs:
            counts["queries_same"] += 1
        else:
            bad.append(f"query {mine.stem}: differs from the one Storm was asked")
    return dict(counts), bad


def compare_runs(rerun: Path, records: Path) -> tuple[dict, list[str]]:
    """Every run of the rerun against the record of the same run."""
    pairs = {"ours": collections.Counter(), "storm": collections.Counter()}
    seconds = {"ours": [0.0, 0.0], "storm": [0.0, 0.0]}
    bad = []
    for folder in sorted(p for p in (rerun / "results").iterdir() if p.is_dir()):
        if analysis.parse_key(folder.name) is None:
            continue
        theirs_folder = records / folder.name
        confs = sorted({f.name[: -len(RECORD_SUFFIX)] for f in theirs_folder.glob("*" + RECORD_SUFFIX)
                        if f.name[: -len(RECORD_SUFFIX)] == STORM
                        or analysis.parse_conf(f.name[: -len(RECORD_SUFFIX)])})
        if not confs:
            bad.append(f"{folder.name}: no record of the campaign")
            continue
        for conf in confs:
            tool = "storm" if conf == STORM else "ours"
            theirs = analysis.load(str(theirs_folder / (conf + RECORD_SUFFIX)))
            mine = analysis.load(str(folder / (conf + RECORD_SUFFIX)))
            if mine is None:
                pairs[tool]["missing"] += 1
                bad.append(f"{folder.name} {conf}: no record in the rerun")
                continue
            a, b = mine.get("status", "failed"), theirs.get("status", "failed")
            pairs[tool][f"{a}/{b}"] += 1
            seconds[tool][0] += analysis.wall_of(mine) or 0.0
            seconds[tool][1] += analysis.wall_of(theirs) or 0.0
            if a == COMPLETE and b == COMPLETE:
                if frontier(conf, mine) == frontier(conf, theirs):
                    pairs[tool]["same_frontier"] += 1
                else:
                    pairs[tool]["other_frontier"] += 1
                    bad.append(f"{folder.name} {conf}: the frontier differs from the record")
    return {"pairs": {k: dict(v) for k, v in pairs.items()}, "seconds": seconds}, bad


def tables(results: Path, baseline: str) -> dict:
    """The numbers of the two tables of the paper on a folder of records."""
    s = analysis.scan(results, baseline)
    storm = {}
    for diag in sorted(s.storm):
        c, b = s.storm[diag], s.baseline[diag]
        storm[diag] = {"instances": c["instances"], "storm_ok": c[COMPLETE],
                       "ours_ok": b[COMPLETE], "both_ok": c["both"],
                       "same": c["same"], "compared": c["compared"],
                       "ratio": analysis.quartiles(s.ratios[diag])["median"]}
    cells = {f"{a}-w{w}": analysis.cell_summary(cell) for (a, w), cell in sorted(s.cells.items())}
    return {"storm": storm, "cells": cells}


def pair(a, b, fmt: str = "{}") -> str:
    """A number of the rerun beside the same number of the records."""
    def one(x):
        return "--" if x is None else fmt.format(x)
    return f"{one(a)} | {one(b)}"


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--rerun", type=Path, required=True,
                   help="the experiment folder of the rerun, as run.py --experiment wrote it")
    p.add_argument("--results", type=Path, required=True,
                   help="the records of the campaign on the same slice, Storm merged in")
    p.add_argument("--instances", type=Path, required=True,
                   help="the instances of the campaign, the folder of the archive of the tool")
    p.add_argument("--models", type=Path, required=True,
                   help="the PRISM models and queries, the folder of the archive of Storm")
    p.add_argument("--baseline", default="onephase-both-w1")
    p.add_argument("--out", type=Path, default=None, help="the report, as JSON")
    a = p.parse_args()

    files, bad_files = compare_files(a.rerun, a.instances, a.models, a.results)
    runs, bad_runs = compare_runs(a.rerun, a.results)
    here, there = tables(a.rerun / "results", a.baseline), tables(a.results, a.baseline)

    print(f"instances: {files.get('instances', 0)} written by the generator, "
          f"{files.get('instances_same', 0)} the same as in the records")
    print(f"models: {files.get('models', 0)} PRISM models, "
          f"{files.get('models_same', 0)} the same as Storm read in the campaign; "
          f"{files.get('queries', 0)} queries, {files.get('queries_same', 0)} the same "
          "as Storm was asked")
    for tool in ("ours", "storm"):
        c = runs["pairs"][tool]
        total = sum(v for k, v in c.items() if "/" in k)
        both = c.get(f"{COMPLETE}/{COMPLETE}", 0)
        print(f"{tool}: {total} runs, complete here and in the records {both}, "
              f"the same frontier on {c.get('same_frontier', 0)} of them")
        others = {k: v for k, v in c.items() if "/" in k and k != f"{COMPLETE}/{COMPLETE}"}
        for k, v in sorted(others.items()):
            mine, theirs = k.split("/")
            print(f"    {v} {mine} here and {theirs} in the records")
        here_s, there_s = runs["seconds"][tool]
        print(f"    wall clock of the runs: {here_s / 3600:.2f} h here, "
              f"{there_s / 3600:.2f} h in the campaign")

    print(f"\nStorm against {a.baseline}, by diagonal, here | in the records")
    print(f"{'diag.':>5} {'instances':>11} {'Storm ok':>11} {'ours ok':>11} {'both ok':>11} "
          f"{'same':>15} {'median ratio':>17}")
    for diag in sorted(set(here["storm"]) | set(there["storm"])):
        h, t = here["storm"].get(diag, {}), there["storm"].get(diag, {})
        same_h = f"{h.get('same', 0)}/{h.get('compared', 0)}"
        same_t = f"{t.get('same', 0)}/{t.get('compared', 0)}"
        print(f"{diag:>5} {pair(h.get('instances'), t.get('instances')):>11} "
              f"{pair(h.get('storm_ok'), t.get('storm_ok')):>11} "
              f"{pair(h.get('ours_ok'), t.get('ours_ok')):>11} "
              f"{pair(h.get('both_ok'), t.get('both_ok')):>11} "
              f"{same_h + ' | ' + same_t:>15} "
              f"{pair(h.get('ratio'), t.get('ratio'), '{:.0f}'):>17}")

    print("\nThe ablation study, here | in the records")
    print(f"{'configuration':>14} {'runs':>11} {'cut':>9} {'points/s':>19} {'apexes/s':>19}")
    for conf in sorted(set(here["cells"]) | set(there["cells"])):
        h, t = here["cells"].get(conf, {}), there["cells"].get(conf, {})
        print(f"{conf:>14} {pair(h.get('runs'), t.get('runs')):>11} "
              f"{pair(h.get('truncated'), t.get('truncated')):>9} "
              f"{pair(h.get('points_per_second'), t.get('points_per_second'), '{:.0f}'):>19} "
              f"{pair(h.get('apexes_per_second'), t.get('apexes_per_second'), '{:.0f}'):>19}")
    print("The times and the rates are those of the machine at hand and are not "
          "compared; the counts of runs closed within the budget follow the speed "
          "of the machine, the frontiers do not.")

    bad = bad_files + bad_runs
    if a.out:
        a.out.write_text(json.dumps({"files": files, "runs": runs, "here": here,
                                     "records": there, "problems": bad},
                                    indent=1, default=str) + "\n")
    if bad:
        print(f"\n{len(bad)} problems, the first of them:")
        for line in bad[:20]:
            print("   ", line)
        return 1
    print("\nRERUN AGREES WITH THE RECORDS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
