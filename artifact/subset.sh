#!/bin/sh
# The representative subset of the evaluation, about five hours and a half
# on one core: the campaign itself, run again by its own driver on a slice of
# the grid, and every run set against the record the campaign left. It runs
# inside the artifact image, with the folder holding the two archives
# mounted:
#
#     docker run --rm -v <folder with the two zips>:/records \
#         sdcpi-tacas27 artifact/subset.sh /records [diagonal] [process ...]
#
# The slice is the first of the ten processes of every cell of the grid on
# the diagonals 2 to 5: 540 instances, every number of impacts and every
# mode, each asked for its frontier by Storm and by the sixteen
# configurations of ours, 9180 runs with the budget of 120 seconds of the
# paper. Storm takes almost all of the time: 4.9 hours on the machine of the
# campaign, of which 4.6 are its 138 runs that the budget cuts, while ours
# closes all of its 8640 runs of the slice in half a minute in all. The last
# diagonal and the processes are arguments: `subset.sh /records 2` is a
# taste of a few minutes, `subset.sh /records 7 2` the second process up to
# the last diagonal of the paper, half a day.
#
# The driver regenerates every instance from the seed of the campaign and
# exports every model, as the campaign did. The instances, the models and the
# queries must be those of the records byte for byte, and every run complete
# here and in the records must give the same frontier, exactly; a run that
# the budget cut on one side only is counted and not judged. The driver keeps
# every record it writes and resumes where it stopped, so a run interrupted
# is taken up again by the same command when artifact/out is a mounted
# folder. Any failure stops the script.
set -e
cd "$(dirname "$0")/.."

RECORDS="${1:-/records}"
DIAGONAL="${2:-5}"
if [ $# -gt 2 ]; then shift 2; PROCESSES="$*"; else PROCESSES=1; fi
TOOLZIP="$RECORDS/sdcpi-tacas-results-tool.zip"
STORMZIP="$RECORDS/sdcpi-tacas-results-prism-storm.zip"
OUT=artifact/out
[ -f "$TOOLZIP" ] || { echo "missing $TOOLZIP"; exit 1; }
[ -f "$STORMZIP" ] || { echo "missing $STORMZIP"; exit 1; }
[ "$DIAGONAL" -ge 2 ] && [ "$DIAGONAL" -le 7 ] || {
    echo "the records cover the diagonals 2 to 7, not $DIAGONAL"; exit 1; }
mkdir -p "$OUT"
START=$(date +%s)

echo "== 1. The campaign on the slice: process $PROCESSES of every cell, diagonals 2 to $DIAGONAL"
python3 scripts/pareto-frontier/run.py --experiment "$OUT/subset" \
    --anytime --timeout 120 --rounds 1 --no-dead-cells \
    --max-diagonal "$DIAGONAL" --processes $PROCESSES

echo "== 2. The records of the campaign on the same slice"
TOOL=""
STORM=""
for d in $(seq 2 "$DIAGONAL"); do
    for n in $(seq 1 10); do
        i=$((d - n))
        [ "$i" -ge 1 ] && [ "$i" -le 10 ] || continue
        for k in $PROCESSES; do
            TOOL="$TOOL sdcpi-tacas-results-tool/results/$n-$i-$k-*/*.r1.json"
            TOOL="$TOOL sdcpi-tacas-results-tool/instances/$n-nested/$i-independent/$k-process_number/*"
            STORM="$STORM sdcpi-tacas-results-prism-storm/results/$n-$i-$k-*/*.r1.json"
            STORM="$STORM sdcpi-tacas-results-prism-storm/models/$n-$i-$k-*"
        done
    done
done
mkdir -p "$OUT/subset-records"
cd "$OUT/subset-records"
set -f  # the patterns are for unzip, not for the shell
unzip -q -o "$TOOLZIP" $TOOL
unzip -q -o "$STORMZIP" $STORM
set +f
rsync -a sdcpi-tacas-results-prism-storm/results/ sdcpi-tacas-results-tool/results/
cd ../../..

echo "== 3. Every run against its record"
python3 scripts/pareto-frontier/compare_records.py \
    --rerun "$OUT/subset" \
    --results "$OUT/subset-records/sdcpi-tacas-results-tool/results" \
    --instances "$OUT/subset-records/sdcpi-tacas-results-tool/instances" \
    --models "$OUT/subset-records/sdcpi-tacas-results-prism-storm/models" \
    --out "$OUT/subset-report.json"

echo "the slice took $(( $(date +%s) - START )) seconds"
echo "SUBSET PASSED"
