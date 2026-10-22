#!/usr/bin/env python3
"""The exact frontier by dynamic programming over sets of vectors, on the very
model that Storm read.

The script is the independent check of the appendix on the experiments: it
takes the PRISM model the campaign exported, has Storm build it into an
explicit MDP (`--exportbuild`, DRN, rational values), and computes the set of
the expected impact vectors of the pure schedulers by backward induction over
sets of vectors, pruned to the minimal elements at every step. No code of the
search of `sdcpi` and no code of the multi-objective engine of Storm is
involved in the numbers it writes: Storm only builds the state space, the same
one its own query read.

Three ways to call it:

  * one model:
        dynamic_programming.py <model.drn | model.prism> [--budget S]
    writes the frontier as JSON on standard output (`--out` for a file). A
    `.prism` argument needs `storm` on the PATH for the build.

  * the agreement of the appendix:
        dynamic_programming.py --agreement --results <dir> --models <dir>
            [--baseline onephase-both-w1] [--budget 120] [--procs N]
            [--out report.json]
    scans the records as `analysis.py` does, our frontier projected on the
    objectives Storm was asked for, selects the instances where the frontier
    of Storm differs from ours (the 471 where it lacks points and the 11 where
    it adds a dominated one) and the instances Storm alone completes (the
    nine), runs the dynamic programming on each with the budget, and compares:
    on a disagreeing instance the exact frontier must equal ours and, projected
    on the objectives of Storm, cover the set of Storm; on an instance Storm
    alone completes the set of Storm must lie on or above the projected exact
    frontier and every candidate of our cut run must lie on or above a point
    of it. Every instance runs in a process of its own, killed when the build
    or the dynamic programming outlives the budget. The summary says how many
    instances completed and how many agreed; the report carries one row per
    instance.

The models are loop free, so the MDP Storm builds is acyclic but for the self
loops of the absorbing states, and the backward induction visits every state
once, in reverse topological order. Everything is exact: the DRN carries
rationals and the arithmetic is `fractions.Fraction`.
"""

from __future__ import annotations

import argparse
import json
import multiprocessing
import multiprocessing.connection
import os
import re
import subprocess
import sys
import tempfile
import time
from fractions import Fraction
from pathlib import Path

RECORD_SUFFIX = ".r1.json"
STORM_CONF = "storm"
COMPLETE = "complete"


# -- the DRN ------------------------------------------------------------------


class Model:
    """The explicit MDP of one DRN file: for every state its actions, each an
    impact vector and a distribution, the initial state and the final ones,
    and the components the impact vectors carry, in their order."""

    def __init__(self) -> None:
        self.actions: list[list[tuple[tuple[Fraction, ...], list[tuple[int, Fraction]]]]] = []
        self.initial: int | None = None
        self.final: set[int] = set()
        self.components: list[int] = []


def parse_drn(path: Path, impacts: re.Pattern = re.compile(r"^impact(\d+)$")) -> Model:
    """The DRN written by `storm --exportbuild`, read into a Model. The reward
    models are reordered by their index, whatever order the header names them."""
    lines = path.read_text().splitlines()
    it = iter(lines)
    order: list[int] | None = None
    for line in it:
        if line.startswith("@reward_models"):
            names = next(it).split()
            keyed = []
            for pos, name in enumerate(names):
                m = impacts.match(name)
                if not m:
                    raise SystemExit(f"{path}: reward model {name} is not an impact")
                keyed.append((int(m.group(1)), pos))
            order = [pos for _, pos in sorted(keyed)]
            components = [index for index, _ in sorted(keyed)]
        elif line.startswith("@model"):
            break
    if order is None:
        raise SystemExit(f"{path}: no reward models")

    def vector(bracket: str) -> tuple[Fraction, ...]:
        parts = [p.strip() for p in bracket.split(",")] if bracket.strip() else []
        if not parts or parts == [""]:
            return tuple(Fraction(0) for _ in order)
        vals = [Fraction(p) for p in parts]
        return tuple(vals[p] for p in order)

    model = Model()
    model.components = components
    state_re = re.compile(r"^state (\d+)(?: \[(.*?)\])?(.*)$")
    action_re = re.compile(r"^\taction (\S+)(?: \[(.*?)\])?")
    branch_re = re.compile(r"^\t\t(\d+) : (.*)$")
    current: list | None = None
    state_reward: tuple[Fraction, ...] = ()
    for line in it:
        if not line.strip():
            continue
        m = state_re.match(line)
        if m:
            sid = int(m.group(1))
            while len(model.actions) <= sid:
                model.actions.append([])
            current = model.actions[sid]
            state_reward = vector(m.group(2) or "")
            labels = (m.group(3) or "").split()
            if "init" in labels:
                model.initial = sid
            if "final" in labels:
                model.final.add(sid)
            continue
        m = action_re.match(line)
        if m and current is not None:
            reward = vector(m.group(2) or "")
            reward = tuple(r + s for r, s in zip(reward, state_reward))
            current.append((reward, []))
            continue
        m = branch_re.match(line)
        if m and current:
            current[-1][1].append((int(m.group(1)), Fraction(m.group(2))))
            continue
    if model.initial is None:
        raise SystemExit(f"{path}: no initial state")
    return model


class BuildCut(Exception):
    pass


def export_drn(prism: Path, drn: Path, timeout: float | None = None) -> None:
    """Storm builds the PRISM model and writes it as an exact DRN; a build
    that outlives the timeout is killed."""
    try:
        run = subprocess.run(
            ["storm", "--prism", str(prism), "--exact", "--buildfull",
             "--exportbuild", str(drn), "drn"],
            capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        raise BuildCut()
    if run.returncode != 0 or not drn.exists():
        raise SystemExit(f"storm failed on {prism}:\n{run.stdout[-2000:]}{run.stderr[-2000:]}")


# -- the dynamic programming --------------------------------------------------


class Budget:
    """A wall clock budget; `None` never expires."""

    def __init__(self, seconds: float | None) -> None:
        self.deadline = None if seconds is None else time.monotonic() + seconds

    def spent(self) -> bool:
        return self.deadline is not None and time.monotonic() > self.deadline


class Cut(Exception):
    pass


def minimal(vectors) -> list[tuple[Fraction, ...]]:
    """The minimal elements of the set, kept as a sorted list: a vector is
    dropped when another is at least as small on every entry."""
    live: list[tuple[Fraction, ...]] = []
    for v in sorted(set(vectors)):
        if any(all(w <= x for w, x in zip(m, v)) for m in live):
            continue
        live = [m for m in live if not all(x <= w for x, w in zip(v, m))]
        live.append(v)
    return live


def merge(left, right, budget: Budget) -> list[tuple[Fraction, ...]]:
    """The sums of one vector from each side, pruned to the minimal elements;
    the budget is read along the way, a single product being the place where
    the time goes."""
    out = []
    count = 0
    for u in left:
        for v in right:
            out.append(tuple(x + y for x, y in zip(u, v)))
        count += len(right)
        if count > 65536:
            count = 0
            if budget.spent():
                raise Cut()
    return minimal(out)


def frontier(model: Model, budget: Budget) -> list[tuple[Fraction, ...]]:
    """The set of the expected impact vectors of the pure schedulers, minimal
    elements only, by backward induction in reverse topological order."""
    n = len(model.actions)
    succs = [sorted({t for _, dist in model.actions[s] for t, _ in dist} - {s})
             for s in range(n)]
    for s in model.final:
        succs[s] = []

    order: list[int] = []            # reverse topological, by iterative DFS
    seen = [0] * n                   # 0 new, 1 open, 2 closed
    stack = [(model.initial, 0)]
    while stack:
        s, i = stack.pop()
        if i == 0:
            if seen[s] == 2:
                continue
            if seen[s] == 1:
                raise SystemExit("the model has a cycle outside the absorbing states")
            seen[s] = 1
        if i < len(succs[s]):
            stack.append((s, i + 1))
            t = succs[s][i]
            if seen[t] == 1:
                raise SystemExit("the model has a cycle outside the absorbing states")
            if seen[t] == 0:
                stack.append((t, 0))
        else:
            seen[s] = 2
            order.append(s)

    uses = [0] * n
    for s in order:
        for t in succs[s]:
            uses[t] += 1

    zero = tuple(Fraction(0) for _ in range(len(model.actions[model.initial][0][0])
                                            if model.actions[model.initial] else 0))
    sets: dict[int, list[tuple[Fraction, ...]]] = {}
    for s in order:
        if budget.spent():
            raise Cut()
        if s in model.final:
            sets[s] = [zero]
            continue
        choices: list[list[tuple[Fraction, ...]]] = []
        for reward, dist in model.actions[s]:
            dist = [(t, p) for t, p in dist if p != 0]
            combined: list[tuple[Fraction, ...]] | None = None
            for t, p in dist:
                part = sets[t]
                scaled = part if p == 1 else [tuple(p * x for x in v) for v in part]
                combined = scaled if combined is None else merge(combined, scaled, budget)
            if combined is None:
                combined = [zero]
            if any(reward):
                combined = [tuple(r + x for r, x in zip(reward, v)) for v in combined]
            choices.append(combined)
        # one action: the merges above left the set minimal and sorted already;
        # a translation and a positive scaling preserve both.
        sets[s] = choices[0] if len(choices) == 1 else minimal(v for c in choices for v in c)
        for t in succs[s]:
            uses[t] -= 1
            if uses[t] == 0 and t != model.initial:
                sets.pop(t, None)
    return sets[model.initial]


def solve(path: Path, budget_seconds: float | None, workdir: Path | None = None) -> dict:
    """One model, PRISM or DRN, to its frontier record."""
    started = time.monotonic()
    if path.suffix == ".prism":
        own = workdir is None
        tmp = Path(tempfile.mkdtemp()) if own else workdir
        drn = tmp / (path.stem + ".drn")
        try:
            export_drn(path, drn)
            model = parse_drn(drn)
        finally:
            if own:
                drn.unlink(missing_ok=True)
                tmp.rmdir()
    else:
        model = parse_drn(path)
    build_seconds = time.monotonic() - started
    try:
        points = frontier(model, Budget(budget_seconds))
        status = COMPLETE
    except Cut:
        points, status = [], "cut"
    return {
        "model": path.name, "status": status, "states": len(model.actions),
        "components": model.components,
        "build_seconds": round(build_seconds, 3),
        "seconds": round(time.monotonic() - started, 3),
        "points_found": len(points),
        "points": [[str(x) for x in v] for v in points],
    }


# -- the agreement of the appendix --------------------------------------------


def load(path: Path) -> dict | None:
    try:
        with open(path) as fh:
            return json.load(fh)
    except (OSError, json.JSONDecodeError):
        return None


def fractions_of(values) -> tuple[Fraction, ...]:
    return tuple(Fraction(str(x)) for x in values)


def covers(a, b) -> bool:
    """Whether every vector of `b` has a vector of `a` at least as small on
    every entry."""
    return all(any(all(x <= y for x, y in zip(m, t)) for m in a) for t in b)


def storm_objectives(storm: dict, components) -> list[int] | None:
    """The components Storm was asked for, read as `analysis.py` reads them:
    named by the record, or the whole of the components when the arity of the
    answer says so, and unknown otherwise. Storm is not asked for a component
    that is zero on every task, so its vectors may be shorter than ours."""
    if storm.get("objectives") is not None:
        return list(storm["objectives"])
    points = storm.get("points") or []
    if components is not None and points and len(points[0]) == len(components):
        return list(components)
    return None


def projected(vectors, positions) -> list[tuple[Fraction, ...]]:
    """The vectors projected on the positions given, pruned again to the
    minimal elements: a projection of a frontier need not be an antichain."""
    return minimal(tuple(v[p] for p in positions) for v in vectors)


def verdict_of(mine, theirs) -> str:
    """How two frontiers stand to one another, in the words of `analysis.py`."""
    a, b = set(mine), set(theirs)
    if a == b:
        return "same"
    if covers(mine, theirs):
        return "ours_superset" if b <= a else "ours_dominates"
    if covers(theirs, mine):
        return "storm_superset" if a <= b else "storm_dominates"
    return "incomparable"


def classify(results: Path, baseline: str):
    """The instances of the appendix, read as `analysis.py` reads them. Where
    both runs complete, our frontier projected on the objectives Storm was
    asked for is set against the set of Storm, and the instance differs when
    the two are not the same set; an instance whose objectives cannot be read
    is not compared. Where Storm completes and our run is cut, the instance is
    one Storm alone completes."""
    differ, alone = [], []
    for folder in sorted(p for p in results.iterdir() if p.is_dir()):
        storm = load(folder / (STORM_CONF + RECORD_SUFFIX))
        ours = load(folder / (baseline + RECORD_SUFFIX))
        if storm is None or ours is None or storm.get("status") != COMPLETE:
            continue
        components = ours.get("components")
        objectives = storm_objectives(storm, components)
        if components is not None and objectives is not None \
                and any(o not in components for o in objectives):
            objectives = None
        job = {"instance": folder.name, "components": components,
               "objectives": objectives,
               "mine": minimal(fractions_of(p["value"]) for p in ours.get("points", [])),
               "theirs": minimal(fractions_of(p) for p in storm.get("points") or [])}
        if ours.get("status") != COMPLETE:
            job["kind"] = "alone"
            alone.append(job)
            continue
        if components is None or objectives is None:
            continue
        at = {c: j for j, c in enumerate(components)}
        verdict = verdict_of(projected(job["mine"], [at[o] for o in objectives]), job["theirs"])
        if verdict != "same":
            job["kind"], job["verdict"] = "differ", verdict
            differ.append(job)
    return differ, alone


def check_child(model_path: str, budget: float | None, conn) -> None:
    """One instance, in a process of its own: Storm builds the model, the
    child tells the parent, and the dynamic programming runs. The budget is
    read inside both phases; the parent kills the child when a phase outlives
    it all the same."""
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            drn = Path(tmp) / (Path(model_path).stem + ".drn")
            export_drn(Path(model_path), drn, timeout=budget)
            model = parse_drn(drn)
    except BuildCut:
        conn.send({"status": "cut", "phase": "build",
                   "build_seconds": round(time.monotonic() - started, 3)})
        return
    except SystemExit as stop:
        conn.send({"status": "error", "error": str(stop)})
        return
    built = time.monotonic()
    conn.send({"phase": "built", "states": len(model.actions),
               "build_seconds": round(built - started, 3)})
    try:
        points = frontier(model, Budget(budget))
    except Cut:
        conn.send({"status": "cut", "phase": "dp",
                   "seconds": round(time.monotonic() - built, 3)})
        return
    conn.send({"status": COMPLETE, "seconds": round(time.monotonic() - built, 3),
               "components": model.components, "points": points})


def judge(job: dict, result: dict) -> dict:
    """The row of one instance: the exact frontier set against ours on every
    component, and against the set of Storm on the objectives Storm was asked
    for."""
    row = {"instance": job["instance"], "kind": job["kind"],
           "status": result["status"]}
    if "verdict" in job:
        row["verdict"] = job["verdict"]
    for key in ("phase", "states", "build_seconds", "seconds", "error", "killed"):
        if key in result:
            row[key] = result[key]
    if result["status"] != COMPLETE:
        return row
    exact = minimal(result["points"])
    row["points_found"] = len(exact)
    if job["components"] is not None and list(job["components"]) != list(result["components"]):
        row["status"] = "error"
        row["error"] = (f"the record names the components {job['components']}, "
                        f"the model {result['components']}")
        return row
    if job["objectives"] is not None:
        at = {c: j for j, c in enumerate(result["components"])}
        exact_storm = projected(exact, [at[o] for o in job["objectives"]])
        row["storm_contained"] = set(job["theirs"]) <= set(exact_storm)
        row["storm_covered"] = covers(exact_storm, job["theirs"])
    if job["kind"] == "differ":
        row["equal_to_ours"] = set(exact) == set(job["mine"])
    else:
        row["candidates_above"] = covers(exact, job["mine"])
    return row


def supervise(jobs: list[dict], budget: float | None, procs: int, grace: float = 10.0):
    """Run every job in a process of its own, at most `procs` at a time, and
    kill a process whose build or whose dynamic programming outlives the
    budget by more than the grace; the rows come back as the jobs end."""
    pending = list(jobs)
    running: dict = {}
    while pending or running:
        while pending and len(running) < procs:
            job = pending.pop(0)
            receive, send = multiprocessing.Pipe(duplex=False)
            proc = multiprocessing.Process(target=check_child,
                                           args=(job["model"], budget, send), daemon=True)
            proc.start()
            send.close()
            now = time.monotonic()
            running[receive] = {"proc": proc, "job": job, "phase": "build", "built": {},
                                "deadline": None if budget is None else now + budget + grace,
                                "since": now}
        for conn in multiprocessing.connection.wait(list(running), timeout=0.5):
            entry = running[conn]
            try:
                message = conn.recv()
            except EOFError:
                message = {"status": "error",
                           "error": f"the process ended with code {entry['proc'].exitcode}"}
            if message.get("phase") == "built" and "status" not in message:
                now = time.monotonic()
                entry.update(phase="dp", built=message, since=now,
                             deadline=None if budget is None else now + budget + grace)
                continue
            entry["proc"].join()
            conn.close()
            del running[conn]
            yield judge(entry["job"], {**entry["built"], **message})
        now = time.monotonic()
        for conn, entry in list(running.items()):
            if entry["deadline"] is not None and now > entry["deadline"]:
                entry["proc"].kill()
                entry["proc"].join()
                conn.close()
                del running[conn]
                cut = {"status": "cut", "phase": entry["phase"], "killed": True,
                       "seconds": round(now - entry["since"], 3)}
                yield judge(entry["job"], {**entry["built"], **cut})


def agreement(args) -> None:
    differ, alone = classify(args.results, args.baseline)
    verdicts: dict[str, int] = {}
    for job in differ:
        verdicts[job["verdict"]] = verdicts.get(job["verdict"], 0) + 1
    told = ", ".join(f"{v} {n}" for v, n in sorted(verdicts.items()))
    print(f"instances where the frontiers differ: {len(differ)} ({told}); "
          f"Storm alone completes: {len(alone)}", flush=True)
    jobs = []
    for job in differ + alone:
        model = args.models / (job["instance"] + ".prism")
        if not model.exists():
            print(f"  {job['instance']}: no model, skipped", flush=True)
            continue
        job["model"] = str(model)
        jobs.append(job)
    rows = []
    for row in supervise(jobs, args.budget, args.procs):
        rows.append(row)
        flag = ""
        if row["status"] == COMPLETE:
            flag = ("ours agreed" if row.get("equal_to_ours") else
                    "candidates above" if row.get("candidates_above") else "DISAGREES")
            if "storm_covered" in row:
                flag += (", Storm on it" if row["storm_contained"] else
                         ", Storm above it" if row["storm_covered"] else ", STORM BELOW IT")
        elif row["status"] == "cut":
            flag = f"in the {'build' if row.get('phase') == 'build' else 'dynamic programming'}" \
                   + (", killed" if row.get("killed") else "")
        else:
            flag = row.get("error", "")
        seconds = row.get("seconds", row.get("build_seconds", ""))
        print(f"  {row['instance']} [{row['kind']}"
              f"{', ' + row['verdict'] if 'verdict' in row else ''}]: {row['status']} "
              f"{seconds}s {row.get('points_found', '')} points {flag}", flush=True)
    done = [r for r in rows if r["status"] == COMPLETE]
    d_done = [r for r in done if r["kind"] == "differ"]
    a_done = [r for r in done if r["kind"] == "alone"]
    compared = [r for r in done if "storm_covered" in r]
    print(f"completed within the budget: {len(d_done)} of {len(differ)} differing, "
          f"{len(a_done)} of {len(alone)} Storm alone")
    print(f"exact frontier equal to ours: {sum(1 for r in d_done if r['equal_to_ours'])} "
          f"of {len(d_done)}")
    for verdict in sorted(verdicts):
        on = [r for r in d_done if r.get("verdict") == verdict]
        print(f"  {verdict}: {len(on)} completed, the set of Storm on the exact frontier "
              f"on {sum(1 for r in on if r['storm_contained'])}, on or above it on "
              f"{sum(1 for r in on if r['storm_covered'])}")
    print(f"set of Storm on or above the exact frontier, on its objectives: "
          f"{sum(1 for r in compared if r['storm_covered'])} of {len(compared)}")
    if a_done:
        print(f"candidates of the cut runs all on or above the exact frontier: "
              f"{sum(1 for r in a_done if r['candidates_above'])} of {len(a_done)}")
    if args.out:
        args.out.write_text(json.dumps({"baseline": args.baseline, "budget": args.budget,
                                        "rows": rows}, indent=1) + "\n")
        print(f"report: {args.out}")
    bad = [r for r in d_done if not r["equal_to_ours"]] + \
          [r for r in compared if not r["storm_covered"]] + \
          [r for r in a_done if not r["candidates_above"]] + \
          [r for r in rows if r["status"] == "error"]
    if bad:
        raise SystemExit("DISAGREEMENT on: " + ", ".join(sorted({r['instance'] for r in bad})))


# -- entry --------------------------------------------------------------------


def main() -> None:
    here = Path(__file__).resolve().parent
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("model", nargs="?", type=Path,
                   help="one model, .prism (storm builds it) or .drn")
    p.add_argument("--agreement", action="store_true",
                   help="the check of the appendix over the records")
    p.add_argument("--results", type=Path, default=here / "experiment" / "results")
    p.add_argument("--models", type=Path, default=here / "experiment" / "models")
    p.add_argument("--baseline", default="onephase-both-w1")
    p.add_argument("--budget", type=float, default=None,
                   help="seconds for one instance; nothing means no budget")
    p.add_argument("--procs", type=int, default=max(1, (os.cpu_count() or 2) - 1))
    p.add_argument("--out", type=Path, default=None)
    args = p.parse_args()
    if args.agreement:
        agreement(args)
        return
    if not args.model:
        p.error("a model, or --agreement")
    record = solve(args.model, args.budget)
    text = json.dumps(record, indent=1)
    if args.out:
        args.out.write_text(text + "\n")
        print(f"{record['model']}: {record['status']}, {record['points_found']} points, "
              f"{record['seconds']}s -> {args.out}")
    else:
        print(text)


if __name__ == "__main__":
    main()
