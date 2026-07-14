#!/usr/bin/env bash
#
# Fast end-to-end smoke test for the Big Bird pipeline.
#
#   build (release engine)  ->  run one small suite  ->  plot + collect a figure
#
# It caps the Criteo workload to the first N chronological actions (via the
# engine's --max-items flag) so the whole thing finishes in a couple of minutes
# instead of the ~160 s/run the full 1.4M-device workload takes. Output goes to
# a TEMPORARY directory, never the repo's figures/ -- so it can never clobber
# the generated paper figures.
#
#   IMPORTANT: this validates that the pipeline works; it does NOT reproduce the
#   paper's numbers. The capped run is a different, much smaller experiment.
#
# Usage:
#   scripts/smoke.sh                 # temp output dir, default cap
#   scripts/smoke.sh /path/to/out    # explicit output dir
#   BB_SMOKE_MAX_ITEMS=500000 scripts/smoke.sh   # override the workload cap
#
set -euo pipefail

# --- Config ----------------------------------------------------------------
SUITE="attacker-strength-popularity"        # one small suite (10 runs)
MAX_ITEMS="${BB_SMOKE_MAX_ITEMS:-300000}"   # first N chronological actions
THREADS="${BB_SMOKE_THREADS:-$( (nproc 2>/dev/null || echo 8) )}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$REPO_ROOT/bigbirdeval"
BINARY="$CRATE_DIR/target/release/bigbirdeval"

# Temporary output dir (NOT figures/). Caller may pass one explicitly.
if [ "${1:-}" != "" ]; then
    OUT_DIR="$1"
    mkdir -p "$OUT_DIR"
else
    OUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bb_smoke.XXXXXX")"
fi

banner() {
    echo
    echo "============================================================"
    echo "$1"
    echo "============================================================"
}

banner "SMOKE TEST -- validates the pipeline end-to-end; does NOT reproduce paper figures."
echo "Suite:       $SUITE"
echo "Workload cap: first $MAX_ITEMS actions (--max-items)"
echo "Threads:     $THREADS"
echo "Output dir:  $OUT_DIR   (temporary; figures/ is untouched)"

START=$(date +%s)

# --- 1/3 Build -------------------------------------------------------------
banner "[1/3] Building the release engine"
cargo build --release --manifest-path "$CRATE_DIR/Cargo.toml"

# --- 2/3 Run ---------------------------------------------------------------
banner "[2/3] Running $SUITE (capped workload)"
# log4rs.yaml is resolved relative to cwd; keep it + its log/ inside OUT_DIR.
cp "$CRATE_DIR/log4rs.yaml" "$OUT_DIR/log4rs.yaml"
( cd "$OUT_DIR" && "$BINARY" \
    --experiment "$SUITE" \
    --num-threads "$THREADS" \
    --max-items "$MAX_ITEMS" \
    --output-dir "$OUT_DIR" \
    --force )

# --- 3/3 Plot + collect ----------------------------------------------------
banner "[3/3] Plotting + collecting a figure"
LOGS_DIR="$OUT_DIR/$SUITE/logs"
( cd "$REPO_ROOT" && uv run python -m bigbird.plots.cli \
    --input-dir "$LOGS_DIR" \
    --output-dir "$OUT_DIR/$SUITE" )

PDF_DIR="$OUT_DIR/$SUITE/pdf"
FIRST_PDF="$(find "$PDF_DIR" -maxdepth 1 -name '*.pdf' | sort | head -n1 || true)"
if [ -z "$FIRST_PDF" ]; then
    echo "ERROR: no figure PDF was produced in $PDF_DIR" >&2
    exit 1
fi

ELAPSED=$(( $(date +%s) - START ))

banner "SMOKE TEST PASSED in ${ELAPSED}s -- pipeline works end-to-end."
echo "This does NOT reproduce the paper's numbers (workload was capped to"
echo "$MAX_ITEMS actions). The real figures come from ./reproduce.sh."
echo
echo "Example figure: $FIRST_PDF"
echo "All figures:    $PDF_DIR"
echo "Logs:           $LOGS_DIR"
