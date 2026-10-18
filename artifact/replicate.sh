#!/bin/sh
# The replication of the evaluation: from the two archives of the records
# to every number, table and figure of the paper. It runs inside the
# artifact image, with the folder holding the two archives mounted:
#
#     docker run --rm -v <folder with the two zips>:/records \
#         sdcpi-tacas27 artifact/replicate.sh /records
#
# The script extracts the records (about 4 GB of JSON), runs the analysis
# that wrote the data of the paper, compares its output with the reference
# copy shipped in artifact/data-paper, byte for byte except the field
# generated_from of analysis.json, which names the folder read, and renders
# the figures and the tables. With --buffers it also extracts the anytime
# buffers (7 GB more) and rebuilds the per-point anytime metrics, which
# take a few hours; without it, the shipped metrics are used for the one
# figure that reads them. Any failure stops the script.
set -e
cd "$(dirname "$0")/.."

RECORDS="${1:-/records}"
BUFFERS=0
[ "$2" = "--buffers" ] && BUFFERS=1
TOOLZIP="$RECORDS/sdcpi-tacas-results-tool.zip"
STORMZIP="$RECORDS/sdcpi-tacas-results-prism-storm.zip"
OUT=artifact/out
[ -f "$TOOLZIP" ] || { echo "missing $TOOLZIP"; exit 1; }
[ -f "$STORMZIP" ] || { echo "missing $STORMZIP"; exit 1; }
mkdir -p "$OUT"

if [ -f "$RECORDS/SHA256SUMS" ]; then
    echo "== 0. The checksums of the archives"
    (cd "$RECORDS" && sha256sum -c SHA256SUMS)
fi

echo "== 1. The records of the tool and of Storm, extracted and merged"
cd "$OUT"
unzip -q -o "$TOOLZIP" 'sdcpi-tacas-results-tool/results/*.json'
unzip -q -o "$STORMZIP" 'sdcpi-tacas-results-prism-storm/results/*.r1.json'
rsync -a sdcpi-tacas-results-prism-storm/results/ sdcpi-tacas-results-tool/results/
cd ../..

echo "== 2. The analysis of the paper, one pass over every record"
python3 scripts/pareto-frontier/analysis.py \
    --results "$OUT/sdcpi-tacas-results-tool/results" \
    --out "$OUT/data" --max-diagonal 7

echo "== 3. The data rebuilt against the data of the paper"
python3 - "$OUT/data" artifact/data-paper <<'EOF'
import json, sys
from pathlib import Path
new, ref = Path(sys.argv[1]), Path(sys.argv[2])
bad = 0
for r in sorted(ref.iterdir()):
    if r.name == "anytime-metrics.json":
        continue  # rebuilt only with --buffers, compared there
    n = new / r.name
    if not n.exists():
        print("MISSING", r.name); bad += 1; continue
    if r.name == "analysis.json":
        a, b = json.loads(n.read_text()), json.loads(r.read_text())
        a.pop("generated_from", None); b.pop("generated_from", None)
        same = a == b
    else:
        same = n.read_bytes() == r.read_bytes()
    print("ok " if same else "DIFFERS", r.name)
    bad += 0 if same else 1
sys.exit(1 if bad else 0)
EOF

if [ "$BUFFERS" = 1 ]; then
    echo "== 4. The anytime buffers and the per-point metrics (hours)"
    cd "$OUT"
    unzip -q -o "$TOOLZIP" 'sdcpi-tacas-results-tool/results/*.buffer.jsonl.gz'
    cd ../..
    python3 scripts/pareto-frontier/anytime_metrics.py \
        --results "$OUT/sdcpi-tacas-results-tool/results" \
        --out "$OUT/data/anytime-metrics.json" --procs 8
    python3 - "$OUT/data/anytime-metrics.json" \
        artifact/data-paper/anytime-metrics.json <<'EOF'
import json, sys
a = json.loads(open(sys.argv[1]).read())
b = json.loads(open(sys.argv[2]).read())
print("anytime metrics", "ok" if a == b else "DIFFER")
sys.exit(0 if a == b else 1)
EOF
else
    cp artifact/data-paper/anytime-metrics.json "$OUT/data/"
fi

echo "== 5. The figures and the tables of the paper"
mkdir -p scripts/data
cp "$OUT"/data/* scripts/data/
python3 scripts/pareto-frontier/make_figures.py
ls scripts/figures scripts/tables

echo "REPLICATION PASSED"
