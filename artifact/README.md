# Artifact of "A Parallel Algorithm for Strategy Synthesis in BPMN+CPI"

**TACAS 2027, submission 5092**: Emanuele Chini, Daniel Amadori, Pietro Sala

The artifact of the tool paper on `sdcpi`: the tool itself, built from this
repository, the generator of the benchmark with its seed, the drivers of the
campaign, the raw records of every run the paper reads, and the scripts that
rebuild the tables, the figures and the numbers of the evaluation from those
records; the few numbers of the appendix read on the records by other means
are named under "What maps to what".

The permanent copy of the artifact is the Zenodo record
[doi:10.5281/zenodo.23587772](https://doi.org/10.5281/zenodo.23587772),
self-contained for the TACAS 2027 virtual machine and for Docker alike; the
repository is public at <https://github.com/PietroSala/sdcpi-parallel>. The
record holds

| file | size | what it is |
|---|---:|---|
| `README.md`, `LICENSE` | | this file and the MIT license |
| `vm-setup.sh` | 2 kB | installs Docker in the virtual machine and loads the image, with no network |
| `sdcpi-tacas27-docker-amd64.tar`, `sdcpi-tacas27-docker-arm64.tar` | 74 MB, 66 MB | the six packages of Ubuntu 26.04 that Docker needs there |
| `sdcpi-tacas27-image-amd64.tar.gz`, `sdcpi-tacas27-image-arm64.tar.gz` | 1.6 GB, 1.7 GB | the artifact image, which `docker load` restores as `sdcpi-tacas27` |
| `sdcpi-tacas-results-tool.zip` | 7.7 GB | the 11 340 instances of the grid and the 181 440 runs of the tool |
| `sdcpi-tacas-results-prism-storm.zip` | 106 MB | the PRISM models, the queries and the 11 340 runs of Storm 1.13.0 |
| `SHA256SUMS` | | the checksums of the two archives of the records |
| `sdcpi-parallel-<commit>.zip` | 117 MB | this repository at the commit the two images are built from |

**Badges applied for**: Available, Functional, Reusable.

## In short

Every step is one command, run in the folder that holds the files of the
record, and its last line says that it passed. The times and the peaks are
those measured in the TACAS 2027 virtual machine for arm64, with its two
cores and 8 GiB of memory, on an Apple M3 Max; memory and disk are in GiB,
as `free -h` and `df -h` print them, sampled every ten seconds, and the
memory is that of the whole virtual machine, its system included.

| step | what it shows | time | memory | disk |
|---|---|---:|---:|---:|
| setup | Docker and the image, installed with no network | 29 s | 2.0 | 6.4 |
| smoke test | the tool, Storm, the generator, the exact check and the figures | 8 s | 2.4 | |
| subset | 9 180 runs of the campaign made again and set against their records | 4.9 h | 2.2 | 0.2 |
| replication | Tables 1 to 3, the figures and the numbers of the analysis, from the records | 2 min | 2.7 | |
| exact check | the frontiers of Appendix A.2, by dynamic programming | 23 min | 5.7 | 1.3 |

## Requirements

Two cores, 8 GiB of memory and 25 GiB of free disk, with no network: the
TACAS 2027 virtual machine as it ships, amd64 or arm64; any other machine
with Docker does as well. The exact check is the only step that needs more
than 3 GiB of memory, 5.7 GiB of the 7.2 the virtual machine shows, its
system included. The files of the record for one architecture take 8.9 GiB
of disk (9.5 GB), and Docker with the image loaded 6.4 GiB more; the
replication reads the records inside the archives and extracts nothing, the
exact check extracts 1.3 GiB and the subset writes 0.2 GiB, so every step
fits in the virtual machine with the files copied into it: with the files
of both architectures copied in, 30 of its 38 GiB were in use at the peak.

## In the TACAS 2027 virtual machine

1. **The files.** From the record, the files of the architecture of the
   virtual machine, which `dpkg --print-architecture` prints, `amd64` or
   `arm64`: `vm-setup.sh`, `sdcpi-tacas27-docker-<arch>.tar` and
   `sdcpi-tacas27-image-<arch>.tar.gz`; the subset, the replication and the
   exact check read `sdcpi-tacas-results-tool.zip`,
   `sdcpi-tacas-results-prism-storm.zip` and `SHA256SUMS` too. Copy them
   into one folder of the virtual machine, here `~/artifact`. A shared
   folder of VirtualBox that holds them does as well (Settings, Shared
   Folders, auto-mount): the virtual machine shows it as
   `/media/sf_<name>`, which root reads and the user `tacas` does not,
   unless added to the group `vboxsf`. Then `sudo cp /media/sf_<name>/*
   ~/artifact/` copies the files, or `/media/sf_<name>` takes the place of
   `~/artifact` in the commands below, the setup being then
   `sudo sh /media/sf_<name>/vm-setup.sh`; every command runs as root.

2. **Docker and the image**, with no network:

       cd ~/artifact
       sudo sh vm-setup.sh

   The password is `tacas`. The script installs the Docker of Ubuntu 26.04
   from the packages in `sdcpi-tacas27-docker-<arch>.tar`: docker.io 29.1.3
   with containerd, runc and three small dependencies, exactly the packages
   that apt installs for docker.io on the clean machine, each with the
   checksum that the index of the archive of Ubuntu gives it. It then loads
   the image of the architecture of the machine, and its last line is
   `SETUP DONE`. With network, `sudo apt-get install docker.io` installs the
   same Docker, and the script then only loads the image.

3. **The steps**, each described in a section of its own below:

       sudo docker run --rm sdcpi-tacas27 artifact/smoke.sh
       sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 artifact/subset.sh /records
       sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 artifact/replicate.sh /records
       sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 artifact/dp-check.sh /records

`docker run --rm sdcpi-tacas27 <command>` starts a container of the image,
runs the command in it from the root of the repository,
`/artifact/sdcpi-parallel`, and removes the container when the command ends;
`-v ~/artifact:/records:ro` shows the folder `~/artifact` inside the
container as `/records`, read only. What a step writes is removed with its
container, unless a folder of the machine is mounted over `artifact/out`:

    mkdir -p ~/out
    sudo docker run --rm -v ~/artifact:/records:ro \
        -v ~/out:/artifact/sdcpi-parallel/artifact/out \
        sdcpi-tacas27 artifact/subset.sh /records

The command `sudo docker run --rm -it sdcpi-tacas27` opens a shell in the
image, with the tool, Storm and every script at hand.

## With Docker on another machine

On a machine with Docker the image of its architecture is loaded once, and
the same steps run as above, with `sudo` wherever the user is not allowed
Docker:

    docker load -i sdcpi-tacas27-image-<arch>.tar.gz
    docker run --rm sdcpi-tacas27 artifact/smoke.sh
    docker run --rm -v <folder of the record>:/records:ro sdcpi-tacas27 artifact/subset.sh /records

and so on. On a Mac with Apple silicon the arm64 image runs natively and
the amd64 image under emulation. The image also rebuilds from the snapshot
of the repository in the record, with network:

    unzip sdcpi-parallel-<commit>.zip && cd sdcpi-parallel-<commit>
    docker build -f artifact/Dockerfile -t sdcpi-tacas27 .

The base of the image is `movesrwth/storm:1.13.0`, pinned by its digest:
Storm 1.13.0, the version the campaign ran, in its Linux build (the
campaign ran the build of Homebrew on macOS). On top of it the image
carries the Rust toolchain that builds `sdcpi` (1.99.0, with the crates of
`Cargo.lock`), the Python of the analysis and of the figures (pinned
versions), and the benchmark `process-impact-benchmarks` at the commit the
generator pins. The Ubuntu packages are those that the archive of Ubuntu
25.10, the release of the base image, serves at build time, so a rebuild
needs that archive online; the images of the record are the reference.

## The smoke test, a minute at most

    sudo docker run --rm sdcpi-tacas27 artifact/smoke.sh

Eight steps, each echoed as it runs: the tool and Storm answer; the running
example of the paper returns its frontier with the strategy of every point;
the frontier is the same with 1 and with 4 workers; the same with and
without the two early tests; Storm answers on the exported PRISM model, with
the flags of the campaign; the generator writes an instance of the benchmark
grid with the seed of the campaign and the tool computes its frontier; the
dynamic programming over sets of vectors computes the same frontier on the
model Storm reads; the figures and the tables of the paper are rendered from
the reference data shipped in `artifact/data-paper`, and copied into
`artifact/out`. The last line is `SMOKE TEST PASSED`.

## The representative subset, about five hours

    sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 artifact/subset.sh /records

The subset is the campaign itself, run again by its own driver
`scripts/pareto-frontier/run.py` on a slice of the grid, and set run by run
against the records of the campaign. The slice is the first of the ten
processes of every cell of the grid on the diagonals 2 to 5, the diagonal
being the nesting plus the independence of the process: 540 instances,
every number of impacts from 2 to 10 and every one of the six modes, each
asked for its frontier by Storm and by the sixteen configurations of ours,
the four combinations of the two early tests at 1, 2, 4 and 8 workers, 9 180
runs with the budget of 120 seconds of the paper. The driver regenerates
every instance from the seed of the campaign and exports every model, as
the campaign did, and `scripts/pareto-frontier/compare_records.py` then
checks that

* the instances, their PRISM models and the queries Storm is asked are the
  files of the records, byte for byte;
* every run of ours complete here and in the records gives the same
  frontier, the same set of vectors in exact rational arithmetic, for every
  configuration;
* wherever Storm completes here, its set lies on the frontier of ours or
  above it, on the objectives Storm was asked for, as the paper finds on
  every instance;
* no run failed: a run that did not complete was cut by the budget.

A run that the budget cut, on either side, is counted and not judged, the
budget cutting where the speed of the machine puts it, and so is a set of
Storm other than the one recorded. The Storm of the image is the Linux
build of 1.13.0 that `movesrwth/storm:1.13.0` carries, and the campaign ran
the build of Homebrew on macOS; where the search of Storm over the pure
strategies misses points of the frontier, the two builds can miss different
ones. The script prints the comparison with Storm by diagonal, the rows of
Table 1 of the paper on the slice, and the ablation study, the rows of
Table 2, from the rerun and from the records side by side, and the last
line is `SUBSET PASSED`.

Storm takes almost all of the time: in the records, 138 of its 540 runs on
the slice are cut at the budget, 4.6 hours of the 4.9 that the slice took on
the machine of the campaign, while the 8 640 runs of ours close in half a
minute in all. The slice shows what Section 6 reads on the whole grid. In
the records Storm completes 402 of the 540 instances, a smaller share as
the diagonal grows, from 53 of 54 to 133 of 216, and ours all of them; where both complete, the two frontiers are the
same on 389 of the 402, the other 13 being among the instances that the
exact check below settles, and the median of the time of Storm over ours
grows from 93 on diagonal 2 to 374 on diagonal 5. On these diagonals every
run of ours closes in milliseconds, so the slice does not show the effect
of the workers and of the two tests, which the paper reads on the
diagonals 6 and 7; those numbers come from the records, in the
replication.

In the virtual machine for arm64 the subset took 4.9 hours, 4.88 of them
Storm. The 540 instances, their models and their queries were those of the
records, and the 8 640 runs of ours gave the recorded frontiers. Storm
completed 404 instances against the 402 of the records, 3 that the budget
cut in the campaign completing here and 1 that completed there cut here,
and wherever it completed its set lay on the frontier of ours or above it.
On 400 of the 401 instances complete on both sides Storm gave the
recorded set; on `1-2-1-4-random` it gave the four points of the frontier
of ours, where the campaign recorded three of them, and six runs of each
Linux build, arm64 and amd64, give the same four.

The last diagonal and the processes are arguments: `subset.sh /records 2`
is a taste of a few minutes, and `subset.sh /records 7 2` runs the second
process up to the last diagonal of the paper, about 17 hours on the machine
of the campaign, 15 of them Storm. The driver keeps every record it writes
and resumes where it stopped, so with a folder mounted over `artifact/out`,
as above, a subset interrupted is taken up again by the same command.

## The replication of the evaluation, a few minutes

    sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 artifact/replicate.sh /records

The script checks the two archives against `SHA256SUMS`, runs
`scripts/pareto-frontier/analysis.py`, the one pass over the 181 440 runs
of the tool and the 11 340 of Storm that wrote the data of the paper, on
the records read inside the archives, and compares every file it writes
with the reference copy in `artifact/data-paper`: byte for byte, except the
field `generated_from` of `analysis.json`, which names what was read. It
then renders the figures (`scripts/figures/`) and the tables
(`scripts/tables/`) of the experimental section and copies them, with the
data, into `artifact/out`. The last line is `REPLICATION PASSED`.

The flag `--buffers` after `/records` also extracts the records and the
anytime buffers of every run, 17 GB, more than the disk of the virtual
machine holds, and rebuilds the per-point anytime metrics
(`anytime-metrics.json`) from them, a few hours on eight cores; without
the flag, the shipped metrics feed the two figures that read them,
`exp-anytime-cdf.pdf` and `exp-anytime-cdf-abl.pdf`.

## The exact check of the frontiers, about twenty minutes

    sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 artifact/dp-check.sh /records

The two paragraphs of Appendix A.2 that read a dynamic programming are
checked by a script of their own, on the same two archives. The script
selects the instances where the frontier of Storm differs from ours, as
`analysis.py` compares them, our frontier projected on the objectives Storm
was asked for (Storm is not asked for a component that is zero on every
task): 482 in the records, 471 where Storm lacks points and 11 where it
adds a dominated one, and the instances Storm alone completes, 9. On each,
Storm builds the very model it read into an explicit MDP and
`scripts/pareto-frontier/dynamic_programming.py` computes the exact frontier
by backward induction over sets of vectors, exact rational arithmetic
throughout and no code of either search involved. Every instance runs in a
process of its own, stopped when the build of the model by Storm or the
dynamic programming outlives the budget, 120 seconds to each of the two;
the budget and the number of processes, one a core up to four by default,
follow `/records` as arguments. Wherever it completes, the exact frontier must
equal our frontier, the set of Storm must lie on it or above it on the
objectives Storm was asked for, and the candidates of our cut runs must lie
on it or above it; the summary prints the counts and the last line is
`DP CHECK PASSED`. On the machine of the campaign all 482 complete, the
build in 65 seconds at most and the dynamic programming in 15, and of the
nine the one of the appendix completes and the other eight are stopped at
the budget; the check took 649 seconds there with six processes, and 1 360
seconds in the virtual machine with two, with the same counts. On a slower
machine an instance may reach the budget, which the summary counts apart:
the check passes when every instance that completes agrees. The appendix of
the paper reports an earlier run of the check, which completed 468 of the
471 and the instance of the appendix in 377 seconds; the script of the
artifact completes all of them with the same agreement.

The instance of the appendix whose exact frontier has 3 378 points is among
the nine, and the summary of the check sets it against the 64 points of
Storm and the 558 candidates of our cut run. To compute its frontier alone:

    sudo docker run --rm -v ~/artifact:/records:ro sdcpi-tacas27 sh -c \
        'unzip -q /records/sdcpi-tacas-results-prism-storm.zip -d /tmp \
             sdcpi-tacas-results-prism-storm/models/2-5-3-2-bagging_remove_divide.prism &&
         python3 scripts/pareto-frontier/dynamic_programming.py --out /tmp/dp.json \
             /tmp/sdcpi-tacas-results-prism-storm/models/2-5-3-2-bagging_remove_divide.prism'

which prints `complete, 3378 points` in well under a minute.

## The campaign from nothing

The records themselves are reproduced by the driver
`scripts/pareto-frontier/run.py`: it generates the grid with
`scripts/storm-comparison-ablation/make_bpmn_cpi.py` (seed 20260815, the
seed of the campaign), exports the PRISM models, and runs the sixteen
configurations of the tool and Storm with a budget of 120 seconds per run,
one run at a time:

    python3 scripts/pareto-frontier/run.py --experiment artifact/out/campaign \
        --anytime --timeout 120 --rounds 1 --no-dead-cells --max-diagonal 7

The times of its runs add up to about ten days on the machine of the
campaign, a MacBook Pro with an Apple M3 Max (16 cores, 128 GB of memory,
macOS 26.6; the first 4 197 runs of Storm on macOS 26.5.2), and its
records take 17 GB on disk; the whole campaign is not part of the
evaluation of the artifact, and the subset runs this command on a slice. The budgeted runs give
comparable times on comparable hardware, and the analysis is deterministic
from the records whatever the machine.

## What maps to what

* The numbers of Section 6 and of the appendix that `analysis.json`
  carries: the rebuilt data, compared byte for byte with the copy the
  paper read.
* The tables of the paper: `scripts/tables/tab-storm.tex` and
  `tab-ablation.tex`, and `table-grid.tex` among the data files.
* The figures of Section 6: `scripts/figures/exp-*.pdf`, under the names
  the sources of the paper include.
* The behaviour claimed of the tool (exact frontier, one strategy per
  point, independence from workers and ablation, the export Storm reads):
  steps 2 to 7 of the smoke test.
* The runs behind the records, made again: the subset, where the frontier
  of every run of ours complete on both sides is the recorded one, and
  every set of Storm lies on the frontier of ours or above it.
* The agreement of the frontiers and the instances Storm alone completes
  (Appendix A.2): `artifact/dp-check.sh`, over
  `scripts/pareto-frontier/dynamic_programming.py`.
* Not rebuilt by a script of the artifact: Tables 4 and 5 of the appendix,
  and the counts the appendix reads on the records directly (the points
  Storm lacks, the sizes of the models, the agreement of the sixteen
  configurations and the speed-up per instance); the records they read are
  in the two archives. The reruns with a budget of 900 seconds that the
  appendix cites are not in the record.

## Reusing the artifact

* **The tool.** `sdcpi pareto <instance>` computes the frontier of an
  instance, with `--workers`, `--ablation`, `--anytime` and `--json` among
  its options; `sdcpi determine <instance> --B a,b,...` answers one budget;
  `sdcpi parse` reads a process written in its grammar, `sdcpi info` and
  `sdcpi bound` describe an instance, and `sdcpi to_prism` and
  `sdcpi to_objective` export the model and the query that Storm reads.
  The README at the root of the repository documents the model, the format
  of an instance, every command and option, and the output; `examples/`
  holds the running example of the paper, `line.cpi`, and two instances in
  YAML. In the image:

      sudo docker run --rm -it sdcpi-tacas27
      sdcpi pareto examples/medium.yaml --workers 2 --print-strategy 1

* **New benchmarks.** The generator
  `scripts/storm-comparison-ablation/make_bpmn_cpi.py` writes the instance
  of any nesting, independence, process, number of impacts and mode
  (`--help`). The driver `scripts/pareto-frontier/run.py` runs Storm and the
  configurations of ours on any part of the grid into a folder of records,
  resuming where it stopped: `--max-diagonal`, `--processes`,
  `--dimensions` and `--modes` choose the part, `--only` the
  configurations, `--timeout` the budget. `analysis.py --results
  <folder>/results --out scripts/data` writes the tables and the data of
  the figures for such a folder; `make_figures.py` then draws the figures
  from `scripts/data`, given also the anytime metrics that
  `anytime_metrics.py --results <folder>/results --out
  scripts/data/anytime-metrics.json` writes, and the anytime curves exist
  only where some run was cut. `compare_records.py` sets any rerun against
  the records, and `dynamic_programming.py <model.prism>` computes the
  exact frontier of a model of this kind on its own.
* **The code.** The tool is Rust, in `src/`, on six crates pinned by
  `Cargo.lock` (`biodivine-lib-bdd` and `oxidd` for the decision diagrams,
  `num-bigint`, `num-integer`, `num-rational` and `num-traits` for the
  arithmetic), which the image holds, so that it builds there with no
  network. Every module opens on what it holds: the frontier in `pareto.rs`, the search
  of several workers over one computation tree in `search.rs`, the two
  bounds in `bound.rs`, the states and the transitions of the semantics in
  `state.rs`, the instance files in `tree.rs`, the arithmetic in `num.rs`,
  the anytime buffer in `anytime.rs`, the export to PRISM in `to_prism.rs`.
  `cargo build --release` builds it into `target/release/sdcpi`, the
  binary the drivers run (the `sdcpi` on the path of the image is a copy
  made when the image was built). `sdcpi test` lists the drivers of the
  experiments, which `sdcpi test <suite> --help` documents; a bounded taste
  of the campaign is `sdcpi test pareto-frontier --experiment /tmp/taste
  --anytime --rounds 1 --max-diagonal 2 --processes 1 --limit 4`.

## Platforms tested

* **arm64**: the TACAS 2027 virtual machine for arm64 (Ubuntu 26.04.1,
  kernel 7.0), its disk as the OVA ships it, run under QEMU 11.1 with the
  hypervisor of macOS on an Apple M3 Max, macOS 26.6, with the two cores
  and the 8 GB the OVA declares and no network, the files of the record
  copied into it. Every step, with the times of the table above.
* **amd64**: the TACAS 2027 virtual machine for amd64, the same way but
  emulated (QEMU TCG) on the same Mac, four cores, 8 GB, no network, the
  files of the record on a shared folder: the setup (278 s), the smoke test
  (128 s), the replication (1 931 s), the subset on diagonal 2 (1 050 s)
  and the exact check (9 010 s, where emulation brings 19 of the 482
  instances to the budget and the 463 that complete all agree). Emulation
  is slow, and its times are not those of an amd64 machine.
* **Docker Desktop** 4.55 (Docker 29.1.3) on the same Mac, both images,
  arm64 natively and amd64 under emulation: the smoke test with no network
  (6 s and 29 s), the replication (110 s and 165 s) and the subset on
  diagonal 2 (172 s and 251 s).

VirtualBox itself was not at hand: the two images of the virtual machine
were booted from their disks under QEMU, with the shared folder of QEMU in
place of the one of VirtualBox.

## License

The tool, the scripts and this artifact are under the MIT license
(`LICENSE`, a copy of the one at the root of the repository).
