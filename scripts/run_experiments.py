"""Run the Big Bird paper experiment suites and plot them.

For each suite this invokes the pre-built ``bigbirdeval`` release binary, then
runs the plotting CLI (``bigbird.plots.cli``) over the resulting logs. Outputs
land under ``experiments/<timestamp>_<name>/<suite>/``. Run
``scripts/collect_figures.py`` afterwards to rename them to the paper's figure
names.

    uv run python scripts/run_experiments.py                 # all paper suites
    uv run python scripts/run_experiments.py --exp paper     # a single suite
    uv run python scripts/run_experiments.py --replot <dir>  # re-plot existing logs

The binary locates the Criteo dataset via its compiled-in crate path
(``CARGO_MANIFEST_DIR`` -> repo root -> ``data/criteo``), so it must be built
from this checkout (``cargo build --release`` in ``bigbirdeval/``, which
``reproduce.sh`` and this script do for you).
"""

import argparse
import datetime
import os
import shutil
import subprocess
import sys
import time

# Suites that produce the paper's figures (see scripts/collect_figures.py).
EXPERIMENTS = [
    "paper",                         # Fig. 5
    "big-bird-bc",                   # Fig. 6a (domain-cap panel)
    "attacker-strength-num-sybils",  # Fig. 6b (sybil-domains panel)
    "attacker-strength-popularity",  # Fig. 6c (site-popularity panel)
]

# Worker threads to start each suite with. Override with BB_THREADS to cap
# CPU/memory; run_experiment retries with fewer threads on failure regardless.
INITIAL_THREADS = int(os.environ.get("BB_THREADS") or os.cpu_count() or 1)


def get_repo_root():
    # scripts/ lives directly under the repo root.
    script_dir = os.path.dirname(os.path.abspath(__file__))
    return os.path.dirname(script_dir)


def run_command(cmd, cwd=None, check=False, env=None):
    print(f"Running: {' '.join(str(c) for c in cmd)}")
    sys.stdout.flush()
    return subprocess.run(cmd, cwd=cwd, check=check, env=env)


def ensure_binary(crate_dir):
    """Return the release binary path, building it if it is missing."""
    binary = os.path.join(crate_dir, "target", "release", "bigbirdeval")
    if not os.path.isfile(binary):
        print("=== Building release binary ===")
        run_command(["cargo", "build", "--release"], cwd=crate_dir, check=True)
    return binary


def run_experiment(experiment, binary, run_dir):
    """Run one suite, retrying with fewer threads on failure (e.g. OOM)."""
    threads = INITIAL_THREADS
    while threads >= 1:
        print(f"\n[{experiment}] Running with {threads} threads...")
        cmd = [
            binary,
            "--experiment", experiment,
            "--num-threads", str(threads),
            "--output-dir", run_dir,
        ]
        start = time.time()
        # cwd=run_dir so log4rs.yaml (copied there) and its log/ output resolve
        # inside the run dir; the dataset is located via the binary's crate path.
        result = run_command(cmd, cwd=run_dir)
        duration = time.time() - start
        if result.returncode == 0:
            print(f"[{experiment}] Success in {duration:.1f}s with {threads} threads.")
            return True
        print(f"[{experiment}] Failed (code {result.returncode}); reducing threads.")
        new_threads = int(threads * 0.75)
        threads = new_threads if new_threads < threads else threads - 1
    print(f"[{experiment}] Failed completely.")
    return False


def plot_experiment(repo_root, run_dir, experiment):
    exp_dir = os.path.join(run_dir, experiment)
    logs_dir = os.path.join(exp_dir, "logs")
    print(f"\n[{experiment}] Generating plots...")
    cmd = [
        "uv", "run", "python", "-m", "bigbird.plots.cli",
        "--input-dir", logs_dir,
        "--output-dir", exp_dir,
    ]
    ok = run_command(cmd, cwd=repo_root).returncode == 0
    if not ok:
        print(f"Plotting failed for {experiment}")
    return ok


def clean_old_plots(exp_dir):
    if not os.path.isdir(exp_dir):
        return
    for fname in os.listdir(exp_dir):
        fpath = os.path.join(exp_dir, fname)
        if fname.lower().endswith(".png") and os.path.isfile(fpath):
            os.remove(fpath)
        elif fname in ("pdf", "csv") and os.path.isdir(fpath):
            shutil.rmtree(fpath)


def resolve_replot_dir(experiments_root, replot):
    if os.path.isdir(replot):
        return replot
    candidates = (
        sorted(
            d
            for d in os.listdir(experiments_root)
            if os.path.isdir(os.path.join(experiments_root, d))
            and d.endswith(replot)
        )
        if os.path.isdir(experiments_root)
        else []
    )
    if not candidates:
        sys.exit(f"Error: no experiment directory matching '{replot}'")
    return os.path.join(experiments_root, candidates[-1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", default=None, help="Suffix for the run directory")
    parser.add_argument(
        "--exp", help="Comma-separated suites to run (default: all paper suites)"
    )
    parser.add_argument("--replot", help="Re-plot an existing run dir (skip running)")
    args = parser.parse_args()

    experiments_to_run = args.exp.split(",") if args.exp else list(EXPERIMENTS)

    repo_root = get_repo_root()
    crate_dir = os.path.join(repo_root, "bigbirdeval")
    experiments_root = os.path.join(repo_root, "experiments")
    print(f"Repo root: {repo_root}")

    # --- Re-plot mode: reuse existing logs, regenerate figures only ----------
    if args.replot:
        run_dir = resolve_replot_dir(experiments_root, args.replot)
        print(f"Re-plotting existing results in: {run_dir}")
        if not args.exp:
            experiments_to_run = sorted(
                item
                for item in os.listdir(run_dir)
                if os.path.isdir(os.path.join(run_dir, item, "logs"))
            )
            print(f"Auto-detected suites: {experiments_to_run}")
        for i, exp in enumerate(experiments_to_run, start=1):
            print(f"\n{'=' * 20} {exp} ({i}/{len(experiments_to_run)}) {'=' * 20}")
            clean_old_plots(os.path.join(run_dir, exp))
            plot_experiment(repo_root, run_dir, exp)
        return

    # --- Fresh run -----------------------------------------------------------
    os.makedirs(experiments_root, exist_ok=True)
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    name = args.name or (
        experiments_to_run[0]
        if len(experiments_to_run) == 1
        else "paper-figures"
    )
    run_dir = os.path.join(experiments_root, f"{timestamp}_{name}")
    os.makedirs(run_dir, exist_ok=True)
    print(f"Artifacts will be stored in {run_dir}")

    binary = ensure_binary(crate_dir)
    # log4rs.yaml is loaded relative to the binary's cwd; keep it and its log/
    # output contained inside the run dir.
    shutil.copy2(
        os.path.join(crate_dir, "log4rs.yaml"),
        os.path.join(run_dir, "log4rs.yaml"),
    )

    for i, exp in enumerate(experiments_to_run, start=1):
        print(f"\n{'=' * 20} {exp} ({i}/{len(experiments_to_run)}) {'=' * 20}")
        if run_experiment(exp, binary, run_dir):
            plot_experiment(repo_root, run_dir, exp)
        else:
            print(f"Experiment {exp} failed completely, skipping plot.")


if __name__ == "__main__":
    main()
