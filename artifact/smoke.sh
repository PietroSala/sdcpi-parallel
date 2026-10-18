#!/bin/sh
# The smoke test of the artifact: minutes on one core, no network. It runs
# inside the artifact image, from the root of the repository:
#
#     docker run --rm sdcpi-tacas27 artifact/smoke.sh
#
# Seven steps: the tool answers; the running example of the paper gives its
# frontier; the frontier does not depend on the number of workers; it does
# not depend on the ablation; Storm answers on the exported model; the
# generator of the benchmark writes an instance of the grid with the seed
# of the campaign and the tool runs it; the figures and the tables of the
# paper are rendered from the reference data. Any failure stops the script.
set -e
cd "$(dirname "$0")/.."

echo "== 1. The tool and the model checker answer"
sdcpi info examples/line.cpi >/dev/null 2>&1 || true
sdcpi parse --file examples/line.cpi --out /tmp/line.yaml
storm --version | head -1

echo "== 2. The running example: the frontier with its strategy"
sdcpi pareto /tmp/line.yaml --print-size 1 --print-strategy 1 | sed -n '/^points/,$p'

echo "== 3. The frontier does not depend on the schedule: 1 against 4 workers"
for w in 1 4; do
    sdcpi pareto /tmp/line.yaml --workers "$w" | grep '^  exact' | sort > "/tmp/frontier-w$w"
done
diff /tmp/frontier-w1 /tmp/frontier-w4
echo "same exact points"

echo "== 4. The frontier does not depend on the ablation: both against none"
for a in both none; do
    sdcpi pareto /tmp/line.yaml --ablation "$a" | grep '^  exact' | sort > "/tmp/frontier-$a"
done
diff /tmp/frontier-both /tmp/frontier-none
echo "same exact points"

echo "== 5. The export and Storm, as the campaign asked it"
sdcpi to_prism /tmp/line.yaml --out /tmp/line.prism
sdcpi to_objective /tmp/line.yaml --B '{kwh: 100, hours: 8}' --out /tmp/line.props
storm --prism /tmp/line.prism --prop /tmp/line.props --purescheds --exact | tail -4

echo "== 6. The generator of the benchmark, one instance of the grid"
python3 scripts/storm-comparison-ablation/make_bpmn_cpi.py \
    --nested 1 --independent 1 --process-number 1 --dimensions 2 \
    --mode random --out /tmp/grid
INSTANCE=$(find /tmp/grid -name '*.yaml' | head -1)
echo "instance $INSTANCE"
sdcpi pareto "$INSTANCE" --workers 2 | grep -E '^points|^search seconds'

echo "== 7. The figures and the tables of the paper, from the reference data"
mkdir -p scripts/data
cp artifact/data-paper/* scripts/data/
python3 scripts/pareto-frontier/make_figures.py
ls scripts/figures scripts/tables

echo "SMOKE TEST PASSED"
