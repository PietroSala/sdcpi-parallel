#!/usr/bin/env python3
"""The frontier campaign: sdcpi pareto against Storm over the journal grid.

    run.py                                  the campaign, round 1, two minutes
    run.py --max-diagonal 3 --limit 4       a taste, four instances
    run.py --round 2                        the timed-out runs again, doubled

Every instance of the grid of the journal paper is regenerated from the
stable seed of stage 1 of storm-comparison-ablation, nothing is copied,
and asked for the frontier of all its expected impacts nine times: once by
Storm on one core, and eight times by sdcpi pareto, at one, two, four and
eight workers with both tests, then the three ablations at one worker,
then the second library at one worker. The order of the campaign is by
diagonal, nesting plus independence, then process, then number of
impacts, then the six modes, then the nine configurations, so that a cut
at any moment leaves a record usable from the smallest instances up.
Every run is recorded as it completes, one JSON per instance and
configuration, and a run whose record exists is never repeated: the
campaign resumes where it stopped. A later round runs again the runs of
the previous round that timed out, with the timeout doubled.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
OLD = REPO / "scripts" / "storm-comparison-ablation"
SDCPI = REPO / "target" / "release" / "sdcpi"

sys.path.insert(0, str(OLD))
import make_bpmn_cpi as gen  # noqa: E402  the generator of stage 1, called and not copied
import setup as old_setup  # noqa: E402  the pinned benchmark

MODES = gen.MODES
SEED = 20260815
WORKERS = [1, 2, 4, 8]

# The nine configurations of one instance, in the order they run.
CONFS: list[tuple[str, list[str]]] = (
    [("storm", [])]
    + [(f"w{w}", ["--workers", str(w)]) for w in WORKERS]
    + [(a, ["--workers", "1", "--ablation", a]) for a in ("accept", "reject", "none")]
    + [("oxidd", ["--workers", "1", "--obdd-library", "oxidd"])]
)


# The child in flight, so that a signal to the driver takes the child with
# it: a Storm left orphaned has no timeout of its own.
_child: subprocess.Popen | None = None


def _die(signum, frame):  # noqa: ARG001
    if _child is not None and _child.poll() is None:
        _child.kill()
    sys.exit(128 + signum)


signal.signal(signal.SIGTERM, _die)
signal.signal(signal.SIGINT, _die)


def call(cmd: list[str], timeout: float) -> tuple[int | None, str, str, bool]:
    """Runs `cmd` with the timeout, killing it past that: the exit code, the
    two streams, and whether it was killed."""
    global _child
    _child = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        out, err = _child.communicate(timeout=timeout)
        return _child.returncode, out, err, False
    except subprocess.TimeoutExpired:
        _child.kill()
        out, err = _child.communicate()
        return None, out or "", err or "", True
    finally:
        _child = None


def key_of(n: int, i: int, k: int, d: int, m: str) -> str:
    return f"{n}-{i}-{k}-{d}-{m}"


def instances(max_diagonal: int | None, dims: list[int], modes: list[str]):
    """The grid in the order of the campaign."""
    for diag in range(2, 21):
        if max_diagonal and diag > max_diagonal:
            return
        for n in range(1, 11):
            i = diag - n
            if not 1 <= i <= 10:
                continue
            for k in range(1, 11):
                for d in dims:
                    for m in modes:
                        yield n, i, k, d, m


def host_info() -> dict:
    def out(cmd: list[str]) -> str:
        try:
            return subprocess.run(cmd, capture_output=True, text=True, timeout=30).stdout.strip()
        except (OSError, subprocess.TimeoutExpired):
            return ""
    return {
        "host": platform.node(),
        "machine": platform.machine(),
        "cpus": os.cpu_count(),
        "sdcpi_commit": out(["git", "-C", str(REPO), "rev-parse", "HEAD"]),
        "storm": out(["storm", "--version"]).splitlines()[0] if out(["storm", "--version"]) else "",
    }


class Campaign:
    def __init__(self, exp: Path, rnd: int, base_timeout: float, host: dict) -> None:
        self.exp = exp
        self.round = rnd
        self.timeout = base_timeout * (2 ** (rnd - 1))
        self.host = host
        self.instances = exp / "instances"
        self.models = exp / "models"
        self.results = exp / "results"
        for d in (self.instances, self.models, self.results):
            d.mkdir(parents=True, exist_ok=True)
        self.log = open(exp / "campaign.log", "a")

    def say(self, line: str) -> None:
        print(line, flush=True)
        self.log.write(line + "\n")
        self.log.flush()

    # -- the instance and its model ----------------------------------------

    def instance(self, n: int, i: int, k: int, d: int, m: str) -> Path:
        key = dict(nested=n, independent=i, process_number=k, dimensions=d, mode=m)
        path = gen.instance_path(self.instances, key, ".yaml")
        if not path.exists():
            gen.build(n, i, k, d, m, SEED, self.instances)
        return path

    def model(self, key: str, yaml: Path) -> tuple[Path, str]:
        prism = self.models / f"{key}.prism"
        prop = self.models / f"{key}.prop"
        if not prism.exists():
            subprocess.run([str(SDCPI), "to_prism", str(yaml), "--out", str(prism)], check=True)
        if not prop.exists():
            subprocess.run([str(SDCPI), "to_objective", str(yaml), "--pareto", "true",
                            "--out", str(prop)], check=True)
        return prism, prop.read_text().strip()

    # -- the records ---------------------------------------------------------

    def record_path(self, key: str, conf: str, rnd: int) -> Path:
        return self.results / key / f"{conf}.r{rnd}.json"

    def due(self, key: str, conf: str) -> bool:
        """Whether this run is owed in this round."""
        if self.record_path(key, conf, self.round).exists():
            return False
        if self.round == 1:
            return True
        prev = self.record_path(key, conf, self.round - 1)
        if not prev.exists():
            return False
        status = json.loads(prev.read_text()).get("status")
        return status in ("timeout", "killed")

    def write(self, key: str, conf: str, record: dict) -> None:
        path = self.record_path(key, conf, self.round)
        path.parent.mkdir(parents=True, exist_ok=True)
        record.update(conf=conf, round=self.round, timeout=self.timeout, host=self.host)
        path.write_text(json.dumps(record, indent=1) + "\n")

    # -- the runs --------------------------------------------------------------

    def run_storm(self, key: str, yaml: Path) -> dict:
        prism, prop = self.model(key, yaml)
        cmd = ["storm", "--prism", str(prism), "--prop", prop, "--purescheds", "--exact"]
        started = time.monotonic()
        code, out, err, killed = call(cmd, self.timeout)
        wall = time.monotonic() - started
        if killed:
            return {"tool": "storm", "status": "timeout", "seconds": wall, "command": cmd,
                    "stdout_tail": (out + err)[-2000:]}
        text = out + err
        points = [[x.strip() for x in m.group(1).split(",")]
                  for m in re.finditer(r"\(([^()]*)\)\s*approx\.", text)]
        checking = re.search(r"Time for model checking: ([0-9.]+)s", text)
        states = re.search(r"States:\s*(\d+)", text)
        found = re.search(r"(\d+) Pareto optimal points found", text)
        status = "complete" if code == 0 and (found or points) else "failed"
        return {
            "tool": "storm", "status": status, "seconds": wall,
            "checking_seconds": float(checking.group(1)) if checking else None,
            "states": int(states.group(1)) if states else None,
            "points": points, "points_found": int(found.group(1)) if found else len(points),
            "command": cmd, "returncode": code,
            "stdout_tail": text[-2000:] if status != "complete" else "",
        }

    def run_ours(self, key: str, conf: str, extra: list[str], yaml: Path) -> dict:
        out = self.results / key / f"{conf}.r{self.round}.pareto.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        cmd = [str(SDCPI), "pareto", str(yaml), "--time-limit", str(self.timeout),
               "--json", str(out)] + extra
        started = time.monotonic()
        code, so, se, killed = call(cmd, self.timeout + 60)
        wall = time.monotonic() - started
        if killed:
            return {"tool": "sdcpi", "status": "killed", "seconds": wall, "command": cmd}
        if code != 0 or not out.exists():
            return {"tool": "sdcpi", "status": "failed", "seconds": wall, "command": cmd,
                    "returncode": code, "stdout_tail": (so + se)[-2000:]}
        record = json.loads(out.read_text())
        out.unlink()
        record.update(tool="sdcpi", wall_seconds=wall, command=cmd)
        return record

    def run_instance(self, n: int, i: int, k: int, d: int, m: str, only: set[str] | None) -> int:
        key = key_of(n, i, k, d, m)
        done = 0
        for conf, extra in CONFS:
            if only and conf not in only:
                continue
            if not self.due(key, conf):
                continue
            yaml = self.instance(n, i, k, d, m)
            record = self.run_storm(key, yaml) if conf == "storm" else self.run_ours(key, conf, extra, yaml)
            self.write(key, conf, record)
            points = record.get("points_found", len(record.get("points", [])))
            self.say(f"{key:32} {conf:8} {record['status']:9} {points:>4} points "
                     f"{record['seconds']:9.3f} s")
            done += 1
        return done


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--experiment", type=Path, default=HERE / "experiment",
                   help="where instances, models and records live (default experiment/)")
    p.add_argument("--round", type=int, default=1,
                   help="round r runs again what timed out in round r-1, timeout doubled")
    p.add_argument("--timeout", type=float, default=120.0, help="seconds per run in round 1")
    p.add_argument("--max-diagonal", type=int, default=None,
                   help="nesting plus independence up to this; default the whole grid")
    p.add_argument("--dimensions", type=int, nargs="+", default=list(range(2, 11)))
    p.add_argument("--modes", nargs="+", default=MODES)
    p.add_argument("--limit", type=int, default=None, help="stop after this many instances")
    p.add_argument("--only", nargs="+", default=None,
                   help="a subset of the configurations: storm w1 w2 w4 w8 accept reject none oxidd")
    a = p.parse_args()

    if not SDCPI.exists():
        print(f"{SDCPI} is missing: cargo build --release first", file=sys.stderr)
        return 2
    if old_setup.fetch_benchmark():
        return 2
    for m in a.modes:
        if m not in MODES:
            p.error(f"unknown mode {m}")
    for c in a.only or []:
        if c not in dict(CONFS):
            p.error(f"unknown configuration {c}")

    campaign = Campaign(a.experiment.resolve(), a.round, a.timeout, host_info())
    campaign.say(f"round {a.round}, timeout {campaign.timeout:.0f} s, "
                 f"{campaign.host['host']} {campaign.host['cpus']} cpus, "
                 f"sdcpi {campaign.host['sdcpi_commit'][:12]}, {campaign.host['storm']}")
    seen = 0
    for n, i, k, d, m in instances(a.max_diagonal, a.dimensions, a.modes):
        if a.limit is not None and seen >= a.limit:
            break
        campaign.run_instance(n, i, k, d, m, set(a.only) if a.only else None)
        seen += 1
    campaign.say(f"round {a.round} over {seen} instances done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
