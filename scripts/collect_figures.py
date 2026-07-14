"""Collect and rename per-suite plot PDFs into a top-level figures/ directory.

The plotting CLI (`bigbird.plots.cli`) emits generic, per-suite output stems
under `experiments/<run>/<suite>/pdf/<stem>.pdf`. The paper, however, refers to
figures by number (Fig. 5a, Fig. 6, ...). This script closes that gap: it copies
the exact PDFs the paper uses into `figures/`, renamed to the paper's figure
names, driven by the explicit, auditable FIGURES map below.

Run after `scripts/run_experiments.py`. Auto-detects the most recent run under
`experiments/` unless `--run-dir` is given.

    uv run python scripts/collect_figures.py [--run-dir experiments/<dir>]
"""

import argparse
import os
import shutil
import sys

# paper figure name -> (experiment suite / output subdir, plot stem). The stem
# is the filename (without extension) the plotting CLI writes into
# <run>/<suite>/pdf/. Figure 6 is assembled separately (COMBINED_FIG6 below).
FIGURES: dict[str, tuple[str, str]] = {
    # --- Figure 5: benign & under-attack error, all from the `paper` suite ---
    "fig5a_benign_error_no_attack": ("paper", "no_attack_eps_qimp_p50_95"),
    "fig5b_error_causes_no_attack": ("paper", "no_attack_bb_causes_eps_qimp"),
    "fig5c_benign_error_under_attack": ("paper", "benign_attacked_eps_qimp_p50_95"),
    "fig5d_error_causes_under_attack": ("paper", "benign_attacked_bb_causes_eps_qimp"),
}

# Figure 6 as it appears in the paper: three attack-resilience panels sharing one
# legend, drawn directly from the per-panel CSVs (see bigbird/plots/combined.py).
# (suite, stem, x_col, x_label) per panel.
COMBINED_FIG6 = (
    "fig6_attack_resilience",
    [
        ("big-bird-bc", "benign_attacked_quota_count_BY_attack_id_p50_95",
         "quota_count", "Domain cap (q)"),
        ("attacker-strength-num-sybils",
         "benign_attacked_num_sybils_BY_attack_id_p50_95",
         "num_sybils", "# Sybils"),
        ("attacker-strength-popularity",
         "benign_attacked_malicious_site_rank_start_BY_attack_id_p50_95",
         "malicious_site_rank_start", "Attacker site rank"),
    ],
)


def repo_root() -> str:
    script_dir = os.path.dirname(os.path.abspath(__file__))
    return os.path.dirname(script_dir)


def latest_run_dir(experiments_root: str) -> str | None:
    if not os.path.isdir(experiments_root):
        return None
    candidates = [
        os.path.join(experiments_root, d)
        for d in os.listdir(experiments_root)
        if os.path.isdir(os.path.join(experiments_root, d))
    ]
    if not candidates:
        return None
    # Most recently modified run directory.
    return max(candidates, key=os.path.getmtime)


def main() -> int:
    root = repo_root()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--run-dir",
        default=None,
        help="Experiment run dir (default: latest under experiments/).",
    )
    parser.add_argument(
        "--figures-dir",
        default=os.path.join(root, "figures"),
        help="Where to write the renamed paper figures (default: figures/).",
    )
    args = parser.parse_args()

    run_dir = args.run_dir or latest_run_dir(os.path.join(root, "experiments"))
    if not run_dir or not os.path.isdir(run_dir):
        print(
            "ERROR: no experiment run directory found. "
            "Run scripts/run_experiments.py first (or pass --run-dir).",
            file=sys.stderr,
        )
        return 1

    figures_dir = args.figures_dir
    os.makedirs(figures_dir, exist_ok=True)
    print(f"Collecting figures from {run_dir}\n           into {figures_dir}\n")

    collected = 0
    missing = []
    for paper_name, (suite, stem) in FIGURES.items():
        src = os.path.join(run_dir, suite, "pdf", f"{stem}.pdf")
        dst = os.path.join(figures_dir, f"{paper_name}.pdf")
        if os.path.isfile(src):
            shutil.copy2(src, dst)
            print(f"  [ok]      {paper_name}.pdf  <-  {suite}/pdf/{stem}.pdf")
            collected += 1
        else:
            print(f"  [MISSING] {paper_name}.pdf  <-  {suite}/pdf/{stem}.pdf")
            missing.append((paper_name, src))

    # Figure 6: the combined attack-resilience figure, generated from CSVs.
    name, panels = COMBINED_FIG6
    csv_panels = [
        (os.path.join(run_dir, suite, "csv", f"{stem}.csv"), x_col, x_label)
        for suite, stem, x_col, x_label in panels
    ]
    if all(os.path.isfile(p) for p, _, _ in csv_panels):
        from bigbird.plots.combined import render_combined

        render_combined(csv_panels, os.path.join(figures_dir, f"{name}.pdf"))
        print(f"  [ok]      {name}.pdf  <-  combined 3-panel (generated)")
        collected += 1
    else:
        print(f"  [MISSING] {name}.pdf  <-  needs big-bird-bc + attacker-strength CSVs")
        missing.append((name, csv_panels[0][0]))

    print(f"\nCollected {collected}/{len(FIGURES) + 1} figures into {figures_dir}")
    if missing:
        print(
            f"\n{len(missing)} figure(s) missing -- the corresponding suite may "
            "not have been run/plotted yet:",
            file=sys.stderr,
        )
        for paper_name, src in missing:
            print(f"  {paper_name}: expected {src}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
