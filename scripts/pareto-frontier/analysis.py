#!/usr/bin/env python3
"""The figures, the tables and the numbers of the experiments section.

    analysis.py                       everything, into the paper data folder
    analysis.py --out /tmp/x          somewhere else
    analysis.py --baseline onephase-both-w2
    analysis.py --archives tool.zip storm.zip    inside the two archives of the record

One pass over the records of the one-phase campaign
(`experiment/results/<instance>/onephase-<ablation>-w<workers>.r1.json`) and
over the records of Storm (`storm.r1.json`) in the same folders, writing into
the output folder

  * the data files of the plots, one curve to a file, a commented header line
    and columns separated by spaces, so that pgfplots reads them as they are:
    `storm-vs-ours.dat` (the cloud of one point to an instance, for a doubly
    logarithmic scale), `time-median-by-diagonal.dat`, `points-per-second-
    <ablation>.dat` and `apexes-per-second-<ablation>.dat` (against the number
    of workers), `anytime-<ablation>.dat` and `anytime-all.dat` (the mean
    anytime curve over the runs cut at the budget);
  * the tables, to be taken in with \\input: `table-storm.tex` (the comparison
    with Storm, by diagonal), `table-ablation.tex` (the ablation study, by
    ablation and number of workers), `table-grid.tex` (the census of the grid);
  * `analysis.json`, carrying every number the tables and the prose quote.

The records of the earlier two-phase loop, `<library>-<ablation>-w<workers>`,
are in the same folders and are not read.

Conventions the whole file keeps to, and every table declares:

  * a run is `complete` when the search closed by itself within the budget and
    `truncated` when the budget cut it; the points of a truncated run are
    CANDIDATES, the minimal elements of what had been found by the cut, and
    every count that carries them says so in a field of its own;
  * the time of a run is the wall clock the driver measured, capped at the
    budget when the run was cut, and the throughputs are read on the time the
    tool reports for itself, which leaves the start of the process out;
  * a vector of ours carries an entry for every impact component and a vector
    of Storm one for every objective it was asked, so ours is projected on
    those objectives before the two frontiers are compared, in exact rational
    arithmetic and never in floating point.

No quantity is written into this file: the axes of the grid, the set of the
configurations and the budget are all read off the records themselves, so that
the same call gives the numbers of the day as the campaign advances.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import re
import statistics
import sys
import zipfile
from fractions import Fraction
from pathlib import Path

TOOL_PREFIX = "onephase-"
STORM_CONF = "storm"
RECORD_SUFFIX = ".r1.json"
CONF_RE = re.compile(r"^" + TOOL_PREFIX + r"(?P<ablation>[a-z]+)-w(?P<workers>\d+)$")
KEY_RE = re.compile(r"^(?P<n>\d+)-(?P<i>\d+)-(?P<k>\d+)-(?P<d>\d+)-(?P<mode>.+)$")
COMPLETE = "complete"
TRUNCATED = "truncated"


# -- reading ----------------------------------------------------------------


def load(path: str) -> dict | None:
    """The record, or nothing when the file is half written: the campaign runs
    while this is read and a record in flight must not stop the pass."""
    try:
        with open(path) as fh:
            return json.load(fh)
    except (OSError, json.JSONDecodeError):
        return None


def parse_key(name: str) -> dict | None:
    """The axes of an instance folder `<n>-<i>-<k>-<d>-<mode>`."""
    m = KEY_RE.match(name)
    if not m:
        return None
    g = m.groupdict()
    axes = {k: int(g[k]) for k in ("n", "i", "k", "d")}
    axes["mode"] = g["mode"]
    axes["diagonal"] = axes["n"] + axes["i"]
    return axes


def parse_conf(name: str) -> tuple[str, int] | None:
    """The ablation and the number of workers of a configuration name."""
    m = CONF_RE.match(name)
    return (m.group("ablation"), int(m.group("workers"))) if m else None


def records_of(folder: os.DirEntry) -> tuple[dict[str, dict], int]:
    """Every record of one instance folder by configuration name, and how many
    files could not be read. The
    configuration is taken from the file name, which the driver writes, and not
    from the field inside: the earliest runs of the campaign carry the name the
    configuration had before the ablations were named."""
    out: dict[str, dict] = {}
    unreadable = 0
    try:
        entries = list(os.scandir(folder.path))
    except OSError:
        return out, unreadable
    for f in entries:
        if not f.name.endswith(RECORD_SUFFIX):
            continue
        conf = f.name[: -len(RECORD_SUFFIX)]
        if conf != STORM_CONF and not parse_conf(conf):
            continue
        rec = load(f.path)
        if rec is None:
            unreadable += 1
        else:
            out[conf] = rec
    return out, unreadable


def folders_of(results: Path):
    """The instance folders of the records, in the order of their names: the
    name of each, and the reading of its records when asked for."""
    for folder in sorted(os.scandir(results), key=lambda e: e.name):
        if folder.is_dir():
            yield folder.name, lambda folder=folder: records_of(folder)


def archives_of(paths: list[Path]):
    """The same instances read straight out of the archives of the campaign,
    with nothing extracted: every member
    `<root>/results/<instance>/<conf>.r1.json` of every archive, a later
    archive taking the place of an earlier one on the same member, as the
    records of Storm are merged into those of the tool when the two are
    extracted. The records extracted weigh 11.6 GB, more than the disk of a
    small virtual machine holds."""
    members: dict[str, dict[str, tuple]] = collections.defaultdict(dict)
    for path in paths:
        z = zipfile.ZipFile(path)
        for info in z.infolist():
            parts = info.filename.split("/")
            if len(parts) == 4 and parts[1] == "results" and parts[3].endswith(RECORD_SUFFIX):
                members[parts[2]][parts[3]] = (z, info)

    def read(files: dict[str, tuple]) -> tuple[dict[str, dict], int]:
        out: dict[str, dict] = {}
        unreadable = 0
        for name, (z, info) in files.items():
            conf = name[: -len(RECORD_SUFFIX)]
            if conf != STORM_CONF and not parse_conf(conf):
                continue
            try:
                out[conf] = json.loads(z.read(info))
            except (OSError, ValueError, zipfile.BadZipFile):
                unreadable += 1
        return out, unreadable

    for name in sorted(members):
        yield name, lambda name=name: read(members[name])


# -- times ------------------------------------------------------------------


def budget_of(rec: dict) -> float | None:
    """The budget the run was given."""
    for field in ("cut_at", "timeout"):
        if rec.get(field) is not None:
            return float(rec[field])
    return None


def capped(seconds: float | None, budget: float | None) -> float | None:
    """A time that never exceeds the budget: a run killed past the budget is
    left on disk with the wall clock of the killing, which is not a time it
    took to answer."""
    if seconds is None:
        return budget
    return min(float(seconds), budget) if budget is not None else float(seconds)


def wall_of(rec: dict) -> float | None:
    """The wall clock of a run, capped at its budget."""
    return capped(rec.get("wall_seconds", rec.get("seconds")), budget_of(rec))


def tool_of(rec: dict) -> float | None:
    """The time the tool reports for itself, capped at its budget."""
    return capped(rec.get("seconds"), budget_of(rec))


# -- frontiers --------------------------------------------------------------


def vector(values, positions) -> tuple[Fraction, ...]:
    """The vector read exactly and projected on the positions given."""
    return tuple(Fraction(str(values[p])) for p in positions)


def minimal(vectors) -> frozenset[tuple[Fraction, ...]]:
    """The minimal elements: a vector is kept when no other is at least as
    small on every entry. A projection of a frontier need not be an antichain,
    so it is pruned again before the two are compared."""
    live: list[tuple[Fraction, ...]] = []
    for v in sorted(set(vectors)):
        if any(all(w <= x for w, x in zip(m, v)) for m in live):
            continue
        live = [m for m in live if not all(x <= w for x, w in zip(v, m))]
        live.append(v)
    return frozenset(live)


def storm_objectives(storm: dict, components) -> list[int] | None:
    """The components Storm was asked for. The earliest records do not name
    them: they are the whole of the components when the arity of the answer
    says so, and unknown otherwise, in which case the frontiers are not
    compared."""
    if storm.get("objectives") is not None:
        return list(storm["objectives"])
    points = storm.get("points") or []
    if components is not None and points and len(points[0]) == len(components):
        return list(components)
    return None


def covers(a, b) -> bool:
    """Whether every vector of `b` has a vector of `a` at least as small on
    every entry: the frontier `a` is at least as good as `b` everywhere."""
    return all(any(all(x <= y for x, y in zip(m, t)) for m in a) for t in b)


def verdict_of(mine, theirs) -> str:
    """How two frontiers stand to one another."""
    if mine == theirs:
        return "same"
    if covers(mine, theirs):
        return "ours_superset" if theirs <= mine else "ours_dominates"
    if covers(theirs, mine):
        return "storm_superset" if mine <= theirs else "storm_dominates"
    return "incomparable"


VERDICTS = ("same", "ours_superset", "ours_dominates", "storm_superset",
            "storm_dominates", "incomparable")


def compare_frontiers(ours: dict, storm: dict) -> str | None:
    """The two frontiers set against one another, ours projected on the
    objectives of Storm and both read as exact rationals. Nothing when the
    comparison cannot be made."""
    components = ours.get("components")
    objectives = storm_objectives(storm, components)
    if components is None or objectives is None:
        return None
    if any(o not in components for o in objectives):
        return None
    at = {c: j for j, c in enumerate(components)}
    mine = minimal(vector(p["value"], [at[o] for o in objectives]) for p in ours.get("points", []))
    theirs = minimal(vector(p, range(len(objectives))) for p in storm.get("points") or [])
    return verdict_of(mine, theirs)


# -- the pass ---------------------------------------------------------------


class Cell:
    """What is accumulated for one ablation and one number of workers."""

    def __init__(self) -> None:
        self.runs = 0
        self.truncated = 0
        self.points = 0
        self.apexes = 0
        self.candidate_points = 0
        self.candidate_apexes = 0
        self.tool_seconds = 0.0
        self.wall_seconds = 0.0
        self.complete_points = 0
        self.complete_apexes = 0
        self.complete_seconds = 0.0
        self.truncated_seconds = 0.0
        self.rates: list[float] = []

    def add(self, rec: dict) -> None:
        points = int(rec.get("points_found") or 0)
        apexes = int(rec.get("apexes") or 0)
        tool = tool_of(rec) or 0.0
        self.runs += 1
        self.points += points
        self.apexes += apexes
        self.tool_seconds += tool
        self.wall_seconds += wall_of(rec) or 0.0
        if rec.get("status") == TRUNCATED:
            self.truncated += 1
            self.candidate_points += points
            self.candidate_apexes += apexes
            self.truncated_seconds += tool
        else:
            self.complete_points += points
            self.complete_apexes += apexes
            self.complete_seconds += tool
        if tool > 0:
            self.rates.append(points / tool)


class Scan:
    """Everything one pass over the records leaves behind."""

    def __init__(self) -> None:
        self.axes = collections.defaultdict(set)
        self.confs: set[str] = set()
        self.folders = collections.Counter()          # instance folders, by diagonal
        self.runs = collections.Counter()             # our runs, by diagonal
        self.cells: dict[tuple[str, int], Cell] = collections.defaultdict(Cell)
        self.storm = collections.defaultdict(collections.Counter)
        self.baseline = collections.defaultdict(collections.Counter)
        self.ratios = collections.defaultdict(list)   # storm wall over ours, by diagonal
        self.times = collections.defaultdict(lambda: collections.defaultdict(list))
        self.scatter: list[dict] = []
        self.curves = collections.defaultdict(list)   # the anytime curves of the cut runs
        self.budgets = collections.Counter()
        self.unreadable = 0
        self.differing: dict[str, list[str]] = {}


def scan(results: Path, baseline: str, max_diagonal: int | None = None,
         storm_max_diagonal: int | None = None, archives: list[Path] | None = None) -> Scan:
    """One pass over every instance folder, up to a diagonal when one is given:
    the campaign sweeps on past the diagonals the paper declares, and the
    records it writes there stay out of the numbers. With archives the folders
    are read inside them and `results` is not read."""
    s = Scan()
    for key, read in archives_of(archives) if archives else folders_of(results):
        axes = parse_key(key)
        if axes is None:
            continue
        if max_diagonal is not None and axes["diagonal"] > max_diagonal:
            continue
        for name, value in axes.items():
            s.axes[name].add(value)
        s.folders[axes["diagonal"]] += 1
        recs, unreadable = read()
        s.unreadable += unreadable
        for conf, rec in recs.items():
            if conf == STORM_CONF:
                continue
            s.confs.add(conf)
            take_run(s, axes, conf, rec)
        if STORM_CONF in recs and not (
                storm_max_diagonal is not None and axes["diagonal"] > storm_max_diagonal):
            take_storm(s, axes, key, recs[STORM_CONF], recs.get(baseline))
    return s


def take_run(s: Scan, axes: dict, conf: str, rec: dict) -> None:
    """One run of ours into the accumulators."""
    ablation, workers = parse_conf(conf)
    s.runs[axes["diagonal"]] += 1
    s.cells[(ablation, workers)].add(rec)
    budget = budget_of(rec)
    if budget is not None:
        s.budgets[budget] += 1
    if rec.get("status") == TRUNCATED and rec.get("curve"):
        # The mean anytime curve reads four fields of a cut run: kept whole,
        # with their candidates, the cut runs held 18 GB by the end of the
        # pass, and with the four fields alone the pass stays under 1 GB.
        slim = {k: rec[k] for k in ("curve", "points_found", "cut_at", "timeout") if k in rec}
        s.curves[ablation].append(slim)
        s.curves["all"].append(slim)


def take_storm(s: Scan, axes: dict, key: str, storm: dict, ours: dict | None) -> None:
    """One instance carrying a record of Storm, against our baseline."""
    diag = axes["diagonal"]
    s.storm[diag]["instances"] += 1
    s.storm[diag][storm.get("status", "failed")] += 1
    if ours is None:
        return
    s.baseline[diag]["records"] += 1
    s.baseline[diag][ours.get("status", "failed")] += 1
    row = {
        "instance": key, "diagonal": diag, "impacts": axes["d"],
        "storm_seconds": wall_of(storm), "storm_complete": storm.get("status") == COMPLETE,
        "ours_seconds": wall_of(ours), "ours_tool_seconds": tool_of(ours),
        "ours_complete": ours.get("status") == COMPLETE,
        "storm_points": storm.get("points_found"), "ours_points": ours.get("points_found"),
    }
    if storm.get("status") != COMPLETE and row["ours_complete"]:
        s.storm[diag]["storm_undone_ours_complete"] += 1
    if row["storm_complete"] and row["ours_complete"]:
        s.storm[diag]["both"] += 1
        s.times[diag]["storm"].append(row["storm_seconds"])
        s.times[diag]["ours"].append(row["ours_seconds"])
        if row["ours_seconds"]:
            row["ratio"] = row["storm_seconds"] / row["ours_seconds"]
            s.ratios[diag].append(row["ratio"])
        verdict = compare_frontiers(ours, storm)
        if verdict is None:
            s.storm[diag]["uncomparable"] += 1
        else:
            s.storm[diag]["compared"] += 1
            s.storm[diag][verdict] += 1
            if verdict != "same":
                s.storm[diag]["differ"] += 1
                s.differing.setdefault(verdict, []).append(key)
    s.scatter.append(row)


# -- the grid ---------------------------------------------------------------


def pairs_of(diagonal: int, n_max: int) -> int:
    """How many pairs of nesting and independence lie on a diagonal."""
    return sum(1 for n in range(1, n_max + 1) if 1 <= diagonal - n <= n_max)


def grid_of(s: Scan) -> dict:
    """The shape of the grid, read off the folders and the records seen."""
    n_max = max(max(s.axes["n"]), max(s.axes["i"]))
    per_pair = len(s.axes["k"]) * len(s.axes["d"]) * len(s.axes["mode"])
    return {
        "nesting_max": n_max, "processes": len(s.axes["k"]),
        "impacts": sorted(s.axes["d"]), "modes": sorted(s.axes["mode"]),
        "instances_per_pair": per_pair, "configurations": len(s.confs),
        "diagonals": sorted(s.folders),
        "budget_seconds": s.budgets.most_common(1)[0][0] if s.budgets else None,
    }


# -- quantiles and shapes ---------------------------------------------------


def quartiles(values: list[float]) -> dict:
    """The three quartiles of a sample, and its size."""
    xs = sorted(v for v in values if v is not None)
    if not xs:
        return {"n": 0, "q1": None, "median": None, "q3": None}
    if len(xs) == 1:
        return {"n": 1, "q1": xs[0], "median": xs[0], "q3": xs[0]}
    q1, _, q3 = statistics.quantiles(xs, n=4, method="inclusive")
    return {"n": len(xs), "q1": q1, "median": statistics.median(xs), "q3": q3}


def rate(top: float, bottom: float) -> float | None:
    """A quotient, and nothing when there is nothing to divide by."""
    return top / bottom if bottom else None


def cell_summary(cell: Cell) -> dict:
    """One row of the ablation study."""
    return {
        "runs": cell.runs, "truncated": cell.truncated,
        "points": cell.points, "apexes": cell.apexes,
        "candidate_points": cell.candidate_points, "candidate_apexes": cell.candidate_apexes,
        "tool_seconds": cell.tool_seconds, "wall_seconds": cell.wall_seconds,
        "truncated_seconds": cell.truncated_seconds,
        "truncated_time_share": rate(cell.truncated_seconds, cell.tool_seconds),
        "points_per_second": rate(cell.points, cell.tool_seconds),
        "apexes_per_second": rate(cell.apexes, cell.tool_seconds),
        "points_per_second_complete": rate(cell.complete_points, cell.complete_seconds),
        "apexes_per_second_complete": rate(cell.complete_apexes, cell.complete_seconds),
        "yield": rate(cell.points, cell.apexes),
        "yield_complete": rate(cell.complete_points, cell.complete_apexes),
        "median_points_per_second": statistics.median(cell.rates) if cell.rates else None,
    }


def anytime(runs: list[dict], steps: int) -> list[dict]:
    """The mean anytime curve of a sample of cut runs, on a regular grid of the
    budget: the points and the apexes each run had at that instant, and the
    part of its own final count the points make. Every run in the sample was
    cut at the budget, so the grid is the same for all of them."""
    if not runs:
        return []
    budget = max(budget_of(r) or 0.0 for r in runs)
    out = []
    for step in range(1, steps + 1):
        t = budget * step / steps
        points, apexes, parts = [], [], []
        for rec in runs:
            seen = [c for c in rec["curve"] if c[0] <= t]
            p = seen[-1][2] if seen else 0
            points.append(p)
            apexes.append(seen[-1][1] if seen else 0)
            final = rec.get("points_found") or 0
            parts.append(p / final if final else 0.0)
        out.append({
            "seconds": t, "runs": len(runs),
            "mean_points": statistics.fmean(points), "median_points": statistics.median(points),
            "mean_apexes": statistics.fmean(apexes),
            "mean_fraction": statistics.fmean(parts),
        })
    return out


# -- writing ----------------------------------------------------------------


def num(x, digits: int = 6) -> str:
    """A number for a data file, kept to significant digits and not to decimal
    places: the times of the small instances are microseconds and a fixed
    number of decimals would write them all as zero, which no logarithmic axis
    accepts. A hole is written `nan`, which pgfplots skips."""
    if x is None:
        return "nan"
    if isinstance(x, bool):
        return str(int(x))
    if isinstance(x, int):
        return str(x)
    return f"{x:.{digits}g}"


def write_dat(path: Path, columns: list[str], rows: list[list], note: str) -> None:
    """A data file of pgfplots: a commented note, a commented header of the
    column names, then the rows separated by spaces."""
    lines = [f"# {note}", "# " + " ".join(columns)]
    lines += [" ".join(num(v) for v in row) for row in rows]
    path.write_text("\n".join(lines) + "\n")


def write_table(path: Path, caption: str, label: str, spec: str,
                header: list[str], rows: list[list[str]], rules: set[int] = frozenset()) -> None:
    """A table of booktabs, no vertical rule, ready for \\input. The body is
    set small: the widest of them does not hold in the measure of the class
    otherwise."""
    out = ["% written by analysis.py, do not edit", "\\begin{table}[htbp]", "\\centering",
           f"\\caption{{{caption}}}", f"\\label{{{label}}}", "\\small",
           f"\\begin{{tabular}}{{{spec}}}", "\\toprule",
           " & ".join(header) + " \\\\", "\\midrule"]
    for j, row in enumerate(rows):
        out.append(" & ".join(row) + " \\\\")
        if j in rules:
            out.append("\\midrule")
    out += ["\\bottomrule", "\\end{tabular}", "\\end{table}"]
    path.write_text("\n".join(out) + "\n")


def si(x, digits: int = 2) -> str:
    """A number for a table cell."""
    if x is None:
        return "--"
    if isinstance(x, int):
        return f"{x:,}".replace(",", "\\,")
    return f"{x:.{digits}f}"


# -- the plots --------------------------------------------------------------


def plot_storm_cloud(out: Path, s: Scan, baseline: str) -> None:
    """The cloud of one point to an instance, Storm against ours."""
    rows = [[r["diagonal"], r["impacts"], r["storm_seconds"], r["ours_seconds"],
             r["ours_tool_seconds"], r["storm_complete"], r["ours_complete"],
             r.get("ratio")] for r in s.scatter]
    write_dat(out / "storm-vs-ours.dat",
              ["diagonal", "impacts", "storm", "ours", "ours_tool",
               "storm_complete", "ours_complete", "ratio"], rows,
              f"one row to an instance carrying a record of Storm; ours is {baseline}; "
              "wall clock in seconds, capped at the budget; the two last flags say "
              "whether each finished within it (a cut run of ours holds candidate points)")


def plot_medians(out: Path, s: Scan) -> None:
    """The quartiles of the two times, diagonal by diagonal."""
    rows = []
    for diag in sorted(s.times):
        st, ou = quartiles(s.times[diag]["storm"]), quartiles(s.times[diag]["ours"])
        ra = quartiles(s.ratios[diag])
        rows.append([diag, st["n"], st["q1"], st["median"], st["q3"],
                     ou["q1"], ou["median"], ou["q3"], ra["median"]])
    write_dat(out / "time-median-by-diagonal.dat",
              ["diagonal", "instances", "storm_q1", "storm_median", "storm_q3",
               "ours_q1", "ours_median", "ours_q3", "ratio_median"], rows,
              "the instances of the diagonal on which Storm and our baseline both "
              "finished within the budget; seconds of wall clock")


def plot_throughput(out: Path, s: Scan, ablations: list[str], workers: list[int]) -> None:
    """The points and the apexes a second, one file to an ablation."""
    for what in ("points", "apexes"):
        for ablation in ablations:
            rows = []
            for w in workers:
                cell = s.cells.get((ablation, w))
                if cell is None:
                    continue
                d = cell_summary(cell)
                rows.append([w, d[f"{what}_per_second"], d[f"{what}_per_second_complete"],
                             cell.runs, cell.truncated])
            write_dat(out / f"{what}-per-second-{ablation}.dat",
                      ["workers", "per_second", "per_second_complete_runs", "runs", "truncated"],
                      rows,
                      f"{what} a second of the ablation {ablation}, over every run recorded: "
                      "the total found over the total of the time the tool reports for itself. "
                      "The first column of rates carries the cut runs, whose points are "
                      "candidates; the second is read on the complete runs alone")


def plot_anytime(out: Path, s: Scan, steps: int) -> None:
    """The mean anytime curve of the runs the budget cut, one file to an
    ablation and one over all of them."""
    for ablation, runs in sorted(s.curves.items()):
        rows = [[c["seconds"], c["mean_points"], c["median_points"], c["mean_apexes"],
                 c["mean_fraction"], c["runs"]] for c in anytime(runs, steps)]
        write_dat(out / f"anytime-{ablation}.dat",
                  ["seconds", "mean_points", "median_points", "mean_apexes",
                   "mean_fraction", "runs"], rows,
                  f"the mean anytime curve over the {len(runs)} runs cut at the budget "
                  f"({'every ablation' if ablation == 'all' else 'the ablation ' + ablation}); "
                  "the points are candidates at every instant, the last column of means being "
                  "the part of its own count at the cut that a run had reached")


# -- the tables -------------------------------------------------------------


def table_storm(out: Path, s: Scan, baseline: str, budget) -> dict:
    """The comparison with Storm by diagonal, and what it is made of."""
    rows, totals = [], collections.Counter()
    for diag in sorted(s.storm):
        c, b, ra = s.storm[diag], s.baseline[diag], quartiles(s.ratios[diag])
        rows.append([str(diag), si(c["instances"]), si(c[COMPLETE]), si(b[COMPLETE]),
                     si(c["both"]), si(ra["q1"]), si(ra["median"]), si(ra["q3"]),
                     f"{c['same']}/{c['compared']}"])
        for field in (("instances", COMPLETE, "both", "compared", "uncomparable",
                       "differ", "timeout", "storm_undone_ours_complete") + VERDICTS):
            totals[field] += c[field]
        totals["ours_complete"] += b[COMPLETE]
        totals["ours_truncated"] += b[TRUNCATED]
    allr = quartiles([r for v in s.ratios.values() for r in v])
    rows.append(["\\textbf{all}", si(totals["instances"]), si(totals[COMPLETE]),
                 si(totals["ours_complete"]), si(totals["both"]), si(allr["q1"]),
                 si(allr["median"]), si(allr["q3"]),
                 f"{totals['same']}/{totals['compared']}"])
    write_table(
        out / "table-storm.tex",
        caption=(f"Storm against {latex_conf(baseline)}, one core each, on the population "
                 f"of the {totals['instances']} instances that carry a record of Storm, which "
                 "are the diagonals Storm has reached and no other. The budget is "
                 f"{si(budget, 0)} seconds and a column marked ok counts the runs that closed "
                 "within it. The ratio is the wall clock of Storm over ours, read on the "
                 f"{totals['both']} instances where both closed; the {totals['ours_truncated']} "
                 "runs of ours that the budget cut hold candidate points and enter no ratio. "
                 "The column of the same frontiers counts the instances where the two are the "
                 "same set, ours projected on the objectives of Storm and compared in exact "
                 "rational arithmetic, out of those where the comparison could be made. On the "
                 f"{totals['differ']} instances where the two differ, ours holds the frontier "
                 f"of Storm and more on {totals['ours_superset']} of them and covers it, every "
                 "point of Storm being dominated by one of ours, on "
                 f"{totals['ours_dominates']} more; the reverse holds on "
                 f"{totals['storm_superset'] + totals['storm_dominates']} and neither on "
                 f"{totals['incomparable']}."),
        label="tab:storm",
        spec="l" + "r" * 7 + "c",
        header=["diag.", "instances", "Storm ok", "ours ok", "both ok",
                "$q_1$", "median", "$q_3$", "same"],
        rows=rows, rules={len(rows) - 2})
    headline = {
        "ratio": allr,
        "ratio_max": max((r for v in s.ratios.values() for r in v), default=None),
        "storm_seconds": quartiles([t for d in s.times for t in s.times[d]["storm"]]),
        "ours_seconds": quartiles([t for d in s.times for t in s.times[d]["ours"]]),
    }
    return {"headline": headline, "by_diagonal": {str(d): dict(s.storm[d], ours_complete=s.baseline[d][COMPLETE],
                                         ours_truncated=s.baseline[d][TRUNCATED],
                                         ratio=quartiles(s.ratios[d]))
                            for d in sorted(s.storm)},
            "total": dict(totals), "ratio": allr,
            "differing_instances": s.differing}


def latex_conf(conf: str) -> str:
    """The name of a configuration, set as code."""
    return "\\texttt{" + conf.replace("_", "\\_") + "}"


def table_ablation(out: Path, s: Scan, ablations: list[str], workers: list[int]) -> dict:
    """The ablation study by ablation and number of workers."""
    rows, data, rules = [], {}, set()
    for ablation in ablations:
        for w in workers:
            cell = s.cells.get((ablation, w))
            if cell is None:
                continue
            d = cell_summary(cell)
            data[f"{ablation}-w{w}"] = d
            rows.append([f"\\texttt{{{ablation}}}", str(w), si(d["runs"]), si(d["truncated"]),
                         si(d["apexes_per_second"], 1), si(d["points_per_second"], 1),
                         si(d["points_per_second_complete"], 1),
                         si(100 * d["yield"], 1), si(d["candidate_points"])])
        rules.add(len(rows) - 1)
    total_runs = sum(v["runs"] for v in data.values())
    total_cut = sum(v["truncated"] for v in data.values())
    total_cand = sum(v["candidate_points"] for v in data.values())
    cut_share = rate(sum(v["truncated_seconds"] for v in data.values()),
                     sum(v["tool_seconds"] for v in data.values()))
    write_table(
        out / "table-ablation.tex",
        caption=(f"The two early tests, switched on and off, over the {total_runs} runs of the "
                 "campaign recorded so far. A rate is the total found over the total of the "
                 "time the tool reports for itself, the start of the process left out. The "
                 f"yield is the part of the apexes inserted that survive on the frontier. The "
                 f"{total_cut} runs the budget cut are counted in the rates, and the points "
                 f"they contribute, {total_cand} of them, are candidates and not verified "
                 "optima: the last column carries them. Those runs take "
                 f"{si(100 * cut_share, 1)} per cent of the whole of the time, so a rate read "
                 "over every run is close to the rate on the hard instances alone; the column "
                 "before the yield reads the rate of the points on the complete runs, where "
                 "every point is a verified optimum."),
        label="tab:ablation",
        spec="llrrrrrrr",
        header=["ablation", "$w$", "runs", "cut", "apexes/s", "points/s",
                "points/s compl.", "yield (\\%)", "cand.\\ points"],
        rows=rows, rules={r for r in rules if r != len(rows) - 1})
    return {"cells": data, "runs": total_runs, "truncated": total_cut,
            "candidate_points": total_cand}


def table_grid(out: Path, s: Scan, grid: dict, workers: list[int]) -> dict:
    """The census of the grid, diagonal by diagonal."""
    rows, by_diag, totals = [], {}, collections.Counter()
    for diag in grid["diagonals"]:
        instances = pairs_of(diag, grid["nesting_max"]) * grid["instances_per_pair"]
        expected = instances * grid["configurations"]
        done = s.runs[diag]
        by_diag[str(diag)] = {"instances": instances, "started": s.folders[diag],
                              "runs_expected": expected, "runs_done": done,
                              "storm_records": s.storm[diag]["instances"]}
        totals["instances"] += instances
        totals["started"] += s.folders[diag]
        totals["runs_expected"] += expected
        totals["runs_done"] += done
        totals["storm_records"] += s.storm[diag]["instances"]
        rows.append([str(diag), si(instances), si(s.folders[diag]), si(expected), si(done),
                     si(100 * done / expected, 1), si(s.storm[diag]["instances"])])
    rows.append(["\\textbf{all}", si(totals["instances"]), si(totals["started"]),
                 si(totals["runs_expected"]), si(totals["runs_done"]),
                 si(100 * totals["runs_done"] / totals["runs_expected"], 1),
                 si(totals["storm_records"])])
    write_table(
        out / "table-grid.tex",
        caption=(f"The census of the grid on the diagonals the campaign has entered. An "
                 f"instance is a nesting, an independence, a process, a number of impacts and "
                 f"a mode; the {grid['configurations']} configurations of ours are the four "
                 f"ablations at {', '.join(str(w) for w in workers)} workers. The diagonals "
                 "that the campaign has not reached carry no row."),
        label="tab:grid",
        spec="lrrrrrr",
        header=["diagonal", "instances", "started", "runs owed", "runs done", "\\%",
                "Storm records"],
        rows=rows, rules={len(rows) - 2})
    return {"by_diagonal": by_diag, "total": dict(totals)}


# -- the run ----------------------------------------------------------------


def main() -> int:
    """One pass, then every file of the output folder."""
    here = Path(__file__).resolve().parent
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--results", type=Path, default=here / "experiment" / "results")
    p.add_argument("--archives", type=Path, nargs="+", default=None,
                   help="the archives of the campaign, the tool and then Storm, read "
                        "as they are with nothing extracted; --results is then not read")
    p.add_argument("--out", type=Path,
                   default=Path.home() / "Documents" / "github" / "cpi-semantics-mdp"
                   / "parallel-paper" / "data")
    p.add_argument("--baseline", default=TOOL_PREFIX + "both-w1",
                   help="the configuration compared with Storm")
    p.add_argument("--steps", type=int, default=24,
                   help="the instants of the mean anytime curve")
    p.add_argument("--max-diagonal", type=int, default=None,
                   help="the last diagonal the paper declares; the records beyond it stay out")
    p.add_argument("--storm-max-diagonal", type=int, default=None,
                   help="the last diagonal Storm has swept entire; its records beyond it stay out")
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)

    s = scan(a.results, a.baseline, a.max_diagonal, a.storm_max_diagonal, a.archives)
    grid = grid_of(s)
    ablations = sorted({parse_conf(c)[0] for c in s.confs})
    workers = sorted({parse_conf(c)[1] for c in s.confs})

    plot_storm_cloud(a.out, s, a.baseline)
    plot_medians(a.out, s)
    plot_throughput(a.out, s, ablations, workers)
    plot_anytime(a.out, s, a.steps)

    summary = {
        "generated_from": " ".join(map(str, a.archives)) if a.archives else str(a.results),
        "baseline": a.baseline,
        "grid": grid,
        "ablations": ablations,
        "workers": workers,
        "records": {"ours": sum(s.runs.values()),
                    "storm": sum(c["instances"] for c in s.storm.values()),
                    "instance_folders": sum(s.folders.values())},
        "note": ("the points of a run cut at the budget are candidates, the minimal elements "
                 "of what had been found by the cut, and are counted apart everywhere"),
        "storm": table_storm(a.out, s, a.baseline, grid["budget_seconds"]),
        "ablation": table_ablation(a.out, s, ablations, workers),
        "census": table_grid(a.out, s, grid, workers),
        "anytime": {k: anytime(v, a.steps) for k, v in sorted(s.curves.items())},
        "unreadable_records": s.unreadable,
    }
    (a.out / "analysis.json").write_text(json.dumps(summary, indent=1, default=str) + "\n")
    print(f"{sum(s.runs.values())} runs of ours and "
          f"{summary['records']['storm']} of Storm, into {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
