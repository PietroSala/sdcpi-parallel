# Artifact of "A Parallel Algorithm for Strategy Synthesis in CPI"

**TACAS 2027, submission 5092** — Emanuele Chini, Daniel Amadori, Pietro Sala

The artifact of the tool paper on `sdcpi`: the tool itself, built from this
repository, the generator of the benchmark with its seed, the drivers of the
campaign, the raw records of every run the paper reads, and the scripts that
produce every number, table and figure of the evaluation.

<!-- DOI: mint the Zenodo record and write it here before submission. -->
The permanent copy of the artifact is the Zenodo record `[DOI to be
minted]`, which holds this repository, the two archives of the records, and
the artifact image saved to a file. The repository is public at
<https://github.com/PietroSala/sdcpi-parallel>.

**Badges applied for**: Available, Functional, Reusable.

## Format and requirements

Of the two formats of the call, this artifact is a **Docker image**. It
needs Docker (any recent version; the image is built for amd64 and arm64,
the machine of the campaign and of the evaluation being amd64), about 7 GB
of disk for the image, and nothing else: no network is needed after the
image is loaded. The full replication reads the two archives of the records,
7.2 GB zipped, and extracts 11 GB beside them. The analysis of the
replication holds every record in memory and peaks at 18 GB: give the
container 20 GB or more, and when Docker runs inside a virtual machine, as
it does on macOS and on Windows, give that memory to the virtual machine in
the settings of Docker.

The image is loaded from the file in the Zenodo record, or rebuilt from the
repository; the two are the same image.

    docker load -i sdcpi-tacas27-image-amd64.tar.gz
    # or, from the root of the repository, with network:
    docker build --platform linux/amd64 -f artifact/Dockerfile -t sdcpi-tacas27 .

The base of the image is `movesrwth/storm:1.13.0`, the version of Storm the
campaign ran; on top of it the image carries the Rust toolchain that builds
`sdcpi` and the Python that runs the analysis and the figures.

## Smoke test, a few minutes

    docker run --rm sdcpi-tacas27 artifact/smoke.sh

Seven steps, each echoed as it runs: the tool and Storm answer; the running
example of the paper returns its frontier with the strategy of every point;
the frontier is the same with 1 and with 4 workers; the same with and
without the two early tests; Storm answers on the exported PRISM model, with
the flags of the campaign; the generator writes an instance of the benchmark
grid with the seed of the campaign and the tool computes its frontier; the
figures and the tables of the paper are rendered from the reference data
shipped in `artifact/data-paper`. The last line is `SMOKE TEST PASSED`.

## Replication of the evaluation, about a quarter of an hour

The raw records are in the two archives of the Zenodo record, beside this
repository:

* `sdcpi-tacas-results-tool.zip`: the 11 340 instances of the grid and the
  181 440 runs of the tool, sixteen configurations on every instance;
* `sdcpi-tacas-results-prism-storm.zip`: the PRISM models, the queries and
  the runs of Storm 1.13.0 on the same instances.

Each archive carries a `README.md` with the layout and the format of its
records and a `MANIFEST.sha256`; `SHA256SUMS` covers the archives
themselves. With the folder that holds the two archives mounted at
`/records`:

    docker run --rm -v <folder with the two zips>:/records \
        sdcpi-tacas27 artifact/replicate.sh /records

The script checks the archives against `SHA256SUMS`, extracts the records,
runs `scripts/pareto-frontier/analysis.py`, the one pass over the 181 440
runs of the tool and the 11 340 of Storm that wrote the data of the paper,
about a minute of computation, and compares every file it writes with the
reference copy in `artifact/data-paper`: byte for byte, except the field
`generated_from` of `analysis.json`, which names the folder read. It then renders the figures
(`scripts/figures/`) and the tables (`scripts/tables/`) of the experimental
section. The last line is `REPLICATION PASSED`.

The flag `--buffers` after `/records` also extracts the anytime buffers of
every run, 7 GB more, and rebuilds the per-point anytime metrics
(`anytime-metrics.json`) from them, a few hours on eight cores; without the
flag, the shipped metrics feed the one figure that reads them.

To keep what the container writes, mount an output folder over
`artifact/out`:

    docker run --rm -v <folder with the two zips>:/records \
        -v <an empty folder>:/artifact/sdcpi-parallel/artifact/out \
        sdcpi-tacas27 artifact/replicate.sh /records

## The campaign from nothing

The records themselves are reproduced by the driver
`scripts/pareto-frontier/run.py`: it generates the grid with
`scripts/storm-comparison-ablation/make_bpmn_cpi.py` (seed 20260815, the
seed of the campaign), exports the PRISM models, and runs the sixteen
configurations of the tool and Storm with a budget of 120 seconds per run.
This is weeks of one machine and is not part of the evaluation of the
artifact; the smoke test runs the generator and both tools on a sample of
one instance. The machine of the campaign was a MacBook Pro (Apple M4 Pro,
14 cores, 48 GB); the budgeted runs give comparable times on comparable
hardware, and the analysis is deterministic from the records whatever the
machine.

## What maps to what

* The numbers quoted in Section 6 and in Appendix A: `analysis.json` of the
  rebuilt data, compared byte for byte with the copy the paper read.
* The tables of the paper: `scripts/tables/tab-storm.tex` and
  `tab-ablation.tex`, and `table-grid.tex` among the data files.
* The figures of Section 6: `scripts/figures/exp-*.pdf`, under the names
  the sources of the paper include.
* The behaviour claimed of the tool (exact frontier, one strategy per
  point, independence from workers and ablation, the export Storm reads):
  steps 2 to 6 of the smoke test.

## License and platforms

The tool, the scripts and this artifact are under the MIT license
(`LICENSE`, a copy of the one at the root of the repository). The image was
built for both architectures on macOS 26 with Docker 29.1, and the smoke
test passes on both, arm64 natively and linux/amd64 under emulation. The
replication was run to its last line on the arm64 image, every comparison
exact, in about a quarter of an hour.
