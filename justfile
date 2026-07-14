set shell := ["bash", "-uc"]

# Regenerate ALL paper figures end-to-end (the one command).
reproduce:
    ./reproduce.sh

# Create the venv and install Python deps + tooling.
setup:
    uv sync

# Build the bigbirdeval Rust engine (debug).
build:
    cd bigbirdeval; cargo build

# Run the Rust test suite.
test:
    cd bigbirdeval; cargo test -- --nocapture

# clippy --fix, nightly fmt, then deny-warnings clippy (needs nightly for fmt).
format:
    cd bigbirdeval; cargo clippy --fix --allow-dirty; cargo +nightly fmt; cargo clippy --tests  -- -D warnings

# Run experiment suites directly, e.g. `just run "paper,big-bird-bc"`.
run command:
    cd bigbirdeval; cargo run --release -- {{command}}

# Run the plotting CLI, e.g. `just plot "--input-dir logs --output-dir out"`.
plot command:
    uv run python -m bigbird.plots.cli {{command}}

# Build the Criteo workload (10-day and 20-day) into data/criteo/ (~169 MB).
generate-dataset:
    uv run bigbird/datasets/criteo/cli.py --start-day 11 --end-day 30 --name 20last
    uv run notebooks/gen_heavy_convsites_json.py 20last

    uv run bigbird/datasets/criteo/cli.py --start-day 1 --end-day 10 --name 10days
    uv run notebooks/gen_heavy_convsites_json.py 10days

# Launch the marimo notebooks (Table 3 lives in notebooks/criteo_stats.py).
marimo:
    uv run marimo edit notebooks
