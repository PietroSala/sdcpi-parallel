# pareto-frontier

The second test suite of the library, `sdcpi test pareto-frontier`: the
frontier of the achievable expected impacts, computed by `sdcpi pareto`
with a strategy per point, against the same frontier computed by the Storm
model checker with pure schedulers and exact arithmetic, over the grid of
instances of the journal paper. The library at the root of the repository
knows nothing of this suite; the scripts ask the built library and Storm,
and everything they generate lands under `experiment/` beside them, which
is never committed.

## The instances

The grid is the one of `storm-comparison-ablation`, regenerated and never
copied: the thousand process shapes of process-impact-benchmarks at the
pinned commit, ten per pair of nesting and independence from one to ten,
completed by stage 1 of that suite, `make_bpmn_cpi.py`, called and not
reimplemented, with the run seed 20260815. The seed derivation of that
stage is stable, one digest per instance from the seed and the key, so a
grid drawn on any machine is the same bytes, and the artifact carries the
generator and the pinned benchmark, not the instances. Dimensions run
from two to ten, the modes are the six of the benchmark.

## The runs

One instance is asked for the frontier of all its expected impacts
thirty-three times, in this order:

| Configuration | Command |
|---|---|
| `storm` | `storm --prism <model> --prop <multi> --purescheds --exact`, one core |
| `<library>-<ablation>-w<workers>` | `sdcpi pareto --obdd-library L --ablation A --workers N` |

with the library `biodivine` or `oxidd`, the ablation `both`, `accept`,
`reject` or `none`, and one, two, four or eight workers: every library
crossed with every ablation and every number of workers, thirty-two runs
of ours. The baseline every other run is compared with is
`biodivine-both-w1`, the one compared with Storm at equal resources.

The model and the property of Storm come from `sdcpi to_prism` and
`sdcpi to_objective --pareto true`. Every run of ours is at the exact tick
with the tightest extraction and the certificate of every refusal, the
defaults of the command, so its points are the frontier itself; Storm runs at its default
precision, and where its set of points differs from ours the precision is
to be tightened until the set is stable, a step of the analysis and not
of the campaign.

The timeout is uniform, two minutes per run in the first round: the
driver enforces it on Storm from outside, and `sdcpi pareto` enforces it on
itself through `--time-limit`, stopping the exploration and reporting the
frontier found so far as partial, no query running past the time left.

## The order and the record

The campaign runs by diagonal, nesting plus independence from two upward,
then by process, then by number of impacts, then by mode, then the nine
configurations, so that a cut at any moment leaves a record usable from
the smallest instances up. Every run is written as it completes, one JSON
per instance and configuration under `experiment/results/<key>/`, named
`<configuration>.r<round>.json`, and a run whose record exists is never
repeated: the campaign resumes where it stopped. The record of a run of
ours is the JSON report of `sdcpi pareto`, points as exact rationals with
the size of the strategy behind each, the query counts and the times in
the search and in the diagrams; the record of a Storm run holds its points
as rationals, its model checking time and the number of states. Every
record names the host, the number of processors, the commit of the library
and the version of Storm.

A later round, `--round 2` and so on, runs again the runs of the previous
round that timed out, with the timeout doubled, and writes them beside the
previous ones.

## Running it

```sh
cargo build --release
python3 scripts/pareto-frontier/run.py                          # the campaign
python3 scripts/pareto-frontier/run.py --max-diagonal 3 --limit 4   # a taste
python3 scripts/pareto-frontier/run.py --round 2                # the timed-out runs again
```

`--experiment DIR` moves the data root, `--dimensions`, `--modes` and
`--only` narrow the grid and the configurations, `--timeout` sets the
seconds of the first round. The requirements are the ones of the first
suite, Python 3.10 with PyYAML and numpy, the built library, and Storm
1.13.0 on the path. The benchmark is cloned on the first run, into
`scripts/storm-comparison-ablation/process-impact-benchmarks`.
