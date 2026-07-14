#!/usr/bin/env bash
#
# reproduce.sh -- THE one command to regenerate all Big Bird (SOSP 2026) figures.
#
# End-to-end pipeline:
#   1. uv sync                         (Python env + tooling)
#   2. build the Criteo dataset        (~169 MB, skipped if data/criteo exists)
#   3. cargo build --release           (the bigbirdeval Rust engine)
#   4. run all paper suites + plot     (scripts/run_experiments.py)
#   5. collect + rename into figures/  (scripts/collect_figures.py)
#
# Usage:
#   ./reproduce.sh                     # full reproduction
#   BB_THREADS=8 ./reproduce.sh        # cap worker threads (default: all cores)
#
# NOTE: full reproduction is COMPUTE-HEAVY. Each of the 4 suites makes one or
# more sequential passes over the full (~1.4M-device) Criteo workload, and the
# engine is both CPU- and memory-hungry. Expect ~25-30 minutes of wall-clock on
# a many-core server and substantial RAM; run_experiments.py automatically
# retries a suite with fewer threads if the OS OOM-kills it. See README.md.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

banner() {
    echo ""
    echo "=================================================================="
    echo ">>> $*"
    echo "=================================================================="
}

# ---------------------------------------------------------------------------
banner "[1/5] uv sync -- Python environment & tooling"
uv sync

# ---------------------------------------------------------------------------
banner "[2/5] Criteo dataset"
if [ -f "data/criteo/20last_impressions.pqt" ]; then
    echo "data/criteo already present -- skipping the ~169 MB build."
    echo "(delete data/criteo to force a rebuild.)"
else
    echo "Building the Criteo workload from HuggingFace (~169 MB)..."
    uv run bigbird/datasets/criteo/cli.py --start-day 11 --end-day 30 --name 20last
    uv run notebooks/gen_heavy_convsites_json.py 20last
    # The "10days" split is NOT needed for any figure -- it is built on demand
    # by notebooks/criteo_stats.py (Table 3 stats), not here.
fi

# ---------------------------------------------------------------------------
banner "[3/5] cargo build --release -- bigbirdeval"
( cd bigbirdeval && cargo build --release )

# ---------------------------------------------------------------------------
banner "[4/5] Run all paper suites and plot"
uv run python scripts/run_experiments.py

# ---------------------------------------------------------------------------
banner "[5/5] Collect + rename figures into figures/"
uv run python scripts/collect_figures.py

# ---------------------------------------------------------------------------
banner "DONE"
cat <<'EOF'
All paper figures are in:  figures/

  figures/fig5a_benign_error_no_attack.pdf     <- paper
  figures/fig5b_error_causes_no_attack.pdf     <- paper
  figures/fig5c_benign_error_under_attack.pdf  <- paper
  figures/fig5d_error_causes_under_attack.pdf  <- paper
  figures/fig6_attack_resilience.pdf           <- big-bird-bc + attacker-strength-{num-sybils,popularity}

Table 3 (filter-config percentiles) is produced manually from
notebooks/criteo_stats.py -- see README.md ("Manual steps").
EOF
