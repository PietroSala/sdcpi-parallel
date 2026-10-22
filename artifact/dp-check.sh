#!/bin/sh
# The exact check of the frontiers, the two paragraphs of Appendix A.2 that
# read the dynamic programming: the instances where the frontier of Storm
# differs from ours, compared as analysis.py compares them, ours projected on
# the objectives Storm was asked for (482 in the records of the campaign),
# and the instances Storm alone completes (9). For each, Storm builds the
# very model it read into an explicit MDP and
# scripts/pareto-frontier/dynamic_programming.py computes the exact frontier
# by backward induction over sets of vectors, every instance in a process of
# its own, stopped when the build or the dynamic programming outlives the
# budget; wherever it completes it must equal our frontier, the set of Storm
# must lie on or above it on the objectives of Storm, and the candidates of
# our cut runs must lie on or above it. It runs inside the artifact image, with the
# folder holding the two archives mounted:
#
#     docker run --rm -v <folder with the two zips>:/records \
#         sdcpi-tacas27 artifact/dp-check.sh /records [budget] [procs]
#
# The budget is per instance and defaults to the 120 seconds of the paper;
# the extraction is selective, the records of the baseline and of Storm with
# the models, 1.2 GB. Any disagreement stops the script.
set -e
cd "$(dirname "$0")/.."

RECORDS="${1:-/records}"
BUDGET="${2:-120}"
PROCS="${3:-4}"
TOOLZIP="$RECORDS/sdcpi-tacas-results-tool.zip"
STORMZIP="$RECORDS/sdcpi-tacas-results-prism-storm.zip"
OUT=artifact/out
[ -f "$TOOLZIP" ] || { echo "missing $TOOLZIP"; exit 1; }
[ -f "$STORMZIP" ] || { echo "missing $STORMZIP"; exit 1; }
mkdir -p "$OUT"

echo "== 1. The baseline records, the records of Storm and the models"
cd "$OUT"
unzip -q -o "$TOOLZIP" 'sdcpi-tacas-results-tool/results/*/onephase-both-w1.r1.json'
unzip -q -o "$STORMZIP" 'sdcpi-tacas-results-prism-storm/results/*.r1.json' \
                        'sdcpi-tacas-results-prism-storm/models/*.prism'
rsync -a sdcpi-tacas-results-prism-storm/results/ sdcpi-tacas-results-tool/results/
cd ../..

echo "== 2. The dynamic programming on every differing instance and on the nine"
mkdir -p "$OUT/data"
python3 scripts/pareto-frontier/dynamic_programming.py --agreement \
    --results "$OUT/sdcpi-tacas-results-tool/results" \
    --models "$OUT/sdcpi-tacas-results-prism-storm/models" \
    --baseline onephase-both-w1 --budget "$BUDGET" --procs "$PROCS" \
    --out "$OUT/data/dp-agreement.json"

echo "DP CHECK PASSED"
