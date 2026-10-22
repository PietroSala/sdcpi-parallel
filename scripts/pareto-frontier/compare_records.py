#!/usr/bin/env python3
"""A rerun of the campaign set against its records, run by run.

    compare_records.py --rerun <experiment folder of run.py> \\
        --results <records> --instances <records> --models <records>

The rerun is the folder that `run.py --experiment` wrote on a slice of the
grid, with its `instances/`, `models/` and `results/`; the three other
folders are the same slice extracted from the two archives of the campaign,
the records of Storm merged into the results of the tool. Every instance of
the slice, in the rerun or in the records, is checked:

  * the instance the generator wrote must be the one of the records, byte
    for byte, and so must the PRISM model and the query that Storm read;
  * a run of ours complete in the rerun and in the records must give the
    same frontier, the same set of vectors read as exact rationals, for
    every configuration;
  * wherever Storm completes in the rerun, its set must lie on the frontier
    of ours or above it, on the objectives Storm was asked for, as the paper
    finds on every instance; the frontier of ours is the baseline of the
    rerun, or of the records when the rerun cut it, the two being equal
    wherever both complete. A set of Storm that differs from the one it
    recorded is counted and named, not judged: the image carries the Linux
    build of Storm 1.13.0 and the campaign ran the build of Homebrew on
    macOS, and where the search of Storm over the pure strategies misses
    points of the frontier the two builds can miss different ones;
  * a run that the budget cut, on either side, is counted and not judged:
    the budget cuts where the speed of the machine puts it, and a run that
    closed a little under the budget in the campaign may be cut here, or the
    reverse. A run that failed in the rerun, a process that died before the
    budget among them, is a problem.

Then the comparison with Storm and the ablation study of the paper on the
slice, computed by `analysis.py` from the rerun and from the records, side
by side. The last line is `RERUN AGREES WITH THE RECORDS`, and the exit
status is not zero when an instance, a model, a query or a frontier of ours
differs, when Storm finds what ours lacks, when a run failed, or when a run
of the slice has no record on one side.
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
VALID = ("same", "ours_superset", "ours_dominates")


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


def cut_by_budget(tool: str, rec: dict) -> bool:
    """Whether a run that did not complete was stopped by the budget, and not
    by a failure. Storm is killed at the budget and recorded as a timeout. A
    run of ours is killed past the budget, or ends on its own past it, and is
    recorded as truncated either way; a process that died before the budget
    leaves a buffer that reads as truncated too, with neither mark."""
    status = rec.get("status")
    if tool == "storm":
        return status == "timeout"
    if status == "killed":
        return True
    return status == "truncated" and (bool(rec.get("killed")) or rec.get("end_status") == COMPLETE)


def complete_run(recs: dict[str, dict], baseline: str) -> dict | None:
    """A complete run of ours among the records of one instance: the
    baseline, or else any configuration that completed."""
    first = recs.get(baseline)
    if first and first.get("status") == COMPLETE:
        return first
    for conf in sorted(recs):
        if conf != STORM and recs[conf].get("status") == COMPLETE:
            return recs[conf]
    return None


def compare_files(rerun: Path, instances: Path, models: Path,
                  records: Path) -> tuple[dict, list[str]]:
    """The instances, the models and the queries of the rerun against those
    of the records, over the files of both sides."""
    counts = collections.Counter()
    bad = []
    mine_root = rerun / "instances"
    rels = sorted({p.relative_to(mine_root) for p in mine_root.rglob("*.yaml")}
                  | {p.relative_to(instances) for p in instances.rglob("*.yaml")})
    for rel in rels:
        same = same_bytes(mine_root / rel, instances / rel)
        counts["instances"] += 1
        counts["instances_same" if same else "instances_differ"] += 1
        if not same:
            where = ("not in the rerun" if not (mine_root / rel).exists()
                     else "not in the records" if same is None else "differs")
            bad.append(f"instance {rel}: {where}")
    stems = sorted({p.stem for p in (rerun / "models").glob("*.prism")}
                   | {p.stem for p in models.glob("*.prism")})
    for stem in stems:
        mine = rerun / "models" / f"{stem}.prism"
        counts["models"] += 1
        same = same_bytes(mine, models / mine.name)
        counts["models_same" if same else "models_differ"] += 1
        if not same:
            where = ("not in the rerun" if not mine.exists()
                     else "not in the records" if same is None else "differs from the one Storm read")
            bad.append(f"model {stem}: {where}")
        theirs = query_of(analysis.load(str(records / stem / (STORM + RECORD_SUFFIX))))
        if theirs is None:
            continue
        ours = mine.with_suffix(".prop")
        counts["queries"] += 1
        if ours.exists() and ours.read_text().strip() == theirs:
            counts["queries_same"] += 1
        else:
            bad.append(f"query {stem}: differs from the one Storm was asked")
    return dict(counts), bad


def confs_of(folder: Path) -> list[str]:
    """The configurations recorded in one instance folder."""
    names = {f.name[: -len(RECORD_SUFFIX)] for f in folder.glob("*" + RECORD_SUFFIX)}
    return sorted(c for c in names if c == STORM or analysis.parse_conf(c))


def compare_runs(rerun: Path, records: Path, baseline: str) -> tuple[dict, list[str]]:
    """Every run of the slice against the record of the same run, and every
    set of Storm in the rerun against the frontier of ours."""
    pairs = {"ours": collections.Counter(), "storm": collections.Counter()}
    seconds = {"ours": [0.0, 0.0], "storm": [0.0, 0.0]}
    other_sets = []
    bad = []
    keys = sorted({p.name for p in (rerun / "results").iterdir()
                   if p.is_dir() and analysis.parse_key(p.name)}
                  | {p.name for p in records.iterdir() if p.is_dir() and analysis.parse_key(p.name)})
    for key in keys:
        confs = confs_of(records / key)
        if not confs:
            bad.append(f"{key}: no record of the campaign")
            continue
        mine_of, theirs_of = {}, {}
        for conf in confs:
            tool = "storm" if conf == STORM else "ours"
            theirs = analysis.load(str(records / key / (conf + RECORD_SUFFIX)))
            if theirs is None:
                bad.append(f"{key} {conf}: the record of the campaign cannot be read")
                continue
            mine = analysis.load(str(rerun / "results" / key / (conf + RECORD_SUFFIX)))
            if mine is None:
                pairs[tool]["missing"] += 1
                bad.append(f"{key} {conf}: no record in the rerun")
                continue
            mine_of[conf], theirs_of[conf] = mine, theirs
            a, b = mine.get("status", "failed"), theirs.get("status", "failed")
            pairs[tool][f"{a}/{b}"] += 1
            seconds[tool][0] += analysis.wall_of(mine) or 0.0
            seconds[tool][1] += analysis.wall_of(theirs) or 0.0
            if a != COMPLETE and not cut_by_budget(tool, mine):
                bad.append(f"{key} {conf}: {a} here, not a cut of the budget")
            if a == COMPLETE and b == COMPLETE:
                same = frontier(conf, mine) == frontier(conf, theirs)
                pairs[tool]["same_frontier" if same else "other_frontier"] += 1
                if not same and tool == "ours":
                    bad.append(f"{key} {conf}: the frontier differs from the record")
        storm = mine_of.get(STORM)
        if not storm or storm.get("status") != COMPLETE:
            continue
        points = storm.get("points") or []
        if not points or len(points) != storm.get("points_found", len(points)):
            bad.append(f"{key} storm: complete with {len(points)} points read of "
                       f"{storm.get('points_found')} found")
        ours = complete_run(mine_of, baseline) or complete_run(theirs_of, baseline)
        if ours is None:
            verdict = "not judged"      # no run of ours completed: the exact check reads these
        else:
            verdict = analysis.compare_frontiers(ours, storm) or "not comparable"
            if verdict not in VALID:
                bad.append(f"{key} storm: {verdict} against the frontier of ours")
        pairs["storm"][f"verdict:{verdict}"] += 1
        recorded = theirs_of.get(STORM)
        if (recorded and recorded.get("status") == COMPLETE
                and frontier(STORM, storm) != frontier(STORM, recorded)):
            theirs_ours = complete_run(theirs_of, baseline)
            other_sets.append({
                "instance": key, "points_here": len(points), "verdict_here": verdict,
                "points_records": len(recorded.get("points") or []),
                "verdict_records": (analysis.compare_frontiers(theirs_ours, recorded)
                                    if theirs_ours else None) or "not compared"})
    return {"pairs": {k: dict(v) for k, v in pairs.items()}, "seconds": seconds,
            "storm_other_sets": other_sets}, bad


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
    if not analysis.parse_conf(a.baseline):
        p.error(f"no configuration {a.baseline}: onephase-<ablation>-w<workers>")

    files, bad_files = compare_files(a.rerun, a.instances, a.models, a.results)
    runs, bad_runs = compare_runs(a.rerun, a.results, a.baseline)
    here, there = tables(a.rerun / "results", a.baseline), tables(a.results, a.baseline)

    print(f"instances: {files.get('instances', 0)} in the slice, "
          f"{files.get('instances_same', 0)} written by the generator as in the records")
    print(f"models: {files.get('models', 0)} PRISM models, "
          f"{files.get('models_same', 0)} the same as Storm read in the campaign; "
          f"{files.get('queries', 0)} queries, {files.get('queries_same', 0)} the same "
          "as Storm was asked")
    for tool in ("ours", "storm"):
        c = runs["pairs"][tool]
        total = sum(v for k, v in c.items() if "/" in k)
        both = c.get(f"{COMPLETE}/{COMPLETE}", 0)
        print(f"{tool}: {total} runs, complete here and in the records {both}, "
              f"the same {'frontier' if tool == 'ours' else 'set of points'} on "
              f"{c.get('same_frontier', 0)} of them")
        if tool == "storm":
            for o in runs["storm_other_sets"]:
                print(f"    another set on {o['instance']}: {o['points_here']} points here, "
                      f"{o['verdict_here']} against ours, {o['points_records']} in the records, "
                      f"{o['verdict_records']}")
        others = {k: v for k, v in c.items() if "/" in k and k != f"{COMPLETE}/{COMPLETE}"}
        for k, v in sorted(others.items()):
            mine, theirs = k.split("/")
            print(f"    {v} {mine} here and {theirs} in the records")
        if tool == "storm":
            judged = {k.split(":", 1)[1]: v for k, v in c.items() if k.startswith("verdict:")}
            held = sum(v for k, v in judged.items() if k in VALID)
            unjudged = judged.pop("not judged", 0)
            print(f"    on or above the frontier of ours wherever Storm completes here: "
                  f"{held} of {sum(judged.values())} "
                  f"({', '.join(f'{k} {v}' for k, v in sorted(judged.items()))})"
                  + (f"; {unjudged} not judged, no run of ours complete" if unjudged else ""))
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
          "of the machine, the frontiers of ours do not.")

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
