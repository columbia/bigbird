"""Turn one suite's logs into its figure CSVs (the gated artifact) and PDFs.

Driver flow: load + column-align every log, keep the benign/attacked/malicious
views, and for each swept parameter emit the parameter-sweep figures (per system
or split by attack/config) plus the Big Bird error-cause chart.

The paper-figure -> (suite, stem) map lives in ``scripts/collect_figures.py``,
which gathers the final PDFs.
"""

import matplotlib.pyplot as plt
import polars as pl

from bigbird.plots.causes import fig_error_causes, get_oob_causes
from bigbird.plots.load import load_dir
from bigbird.plots.sweep import gen_param_sweep_figs
from bigbird.plots.systems import split_df_by_system

# Swept parameter -> x-axis label. A parameter absent from a suite is skipped.
X_PARAMS = [
    ("malicious_site_rank_start", "Attacker site rank"),
    ("num_redirections", "Redirections"),
    ("num_sybils", "# Sybils"),
    ("sybils_per_conversion", "Sybils / conversion"),
    ("sybils_per_conversion_proportion", "Sybil fraction / conversion"),
    ("quota_count", r"Domain cap ($q$)"),
    ("eps_qimp", r"Impression quota ($\epsilon_{imp}$)"),
    ("eps_nc", r"Per-querier cap ($\epsilon_{nc}$)"),
    ("min_batch_size", "Min batch size"),
    ("genuine_first_chance", "Benign-first chance"),
]

PERCENTILES = [(50, 95), (50,)]


def run(input_dir, output_dir):
    """Plot every figure/CSV for one suite. ``input_dir`` holds the *.json.gz
    logs; outputs go to ``output_dir``/{csv,pdf}/."""
    from pathlib import Path

    output_dir = Path(output_dir)
    for sub in ("csv", "pdf"):
        (output_dir / sub).mkdir(parents=True, exist_ok=True)
    suite = output_dir.name

    df = load_dir(input_dir)
    # Fig 5c/5d only: impression quota <= 5, plus the infinity point (-1).
    if "eps_qimp" in df.columns:
        df = df.filter((pl.col("eps_qimp") <= 5) | (pl.col("eps_qimp") == -1))

    views = {
        "no_attack": df.filter(pl.col("attack_id") == 0),
        "benign_all": df.filter(~pl.col("malicious_querier")),
        "benign_attacked": df.filter(
            (pl.col("attack_id") > 0) & ~pl.col("malicious_querier")
        ),
        "malicious": df.filter(pl.col("malicious_querier")),
    }

    for view, view_df in views.items():
        # Legend is hidden on the paper suite's attacked row (shared with the
        # no-attack row above it); figure-only, no effect on the CSV.
        show_legend = not (view == "benign_attacked" and suite == "paper")
        bb, ppa, cm = split_df_by_system(view_df)

        for x_param, x_label in X_PARAMS:
            if x_param not in view_df.columns:
                continue

            for percentiles in PERCENTILES:
                suffix = "_".join(str(p) for p in percentiles)
                figs = gen_param_sweep_figs(
                    view_df, percentiles, x_param, x_label,
                    show_legend=show_legend,
                )
                for fig, csv_df, split_col in figs:
                    by = f"_BY_{split_col}" if split_col else ""
                    stem = f"{view}_{x_param}{by}_p{suffix}"
                    _write(output_dir, stem, fig, csv_df)

            for system_df, name in [(bb, "bb"), (ppa, "ppa"), (cm, "cm")]:
                fig, csv_df = fig_error_causes(
                    get_oob_causes(system_df), x_param, x_label,
                    show_all_legend=(view == "malicious"), show_legend=show_legend,
                )
                _write(output_dir, f"{view}_{name}_causes_{x_param}", fig, csv_df)


def _write(output_dir, stem, fig, csv_df):
    # CSV first: it is the gated output and must not depend on rendering.
    if csv_df is not None:
        csv_df.write_csv(output_dir / "csv" / f"{stem}.csv")
    if fig is not None:
        fig.savefig(output_dir / "pdf" / f"{stem}.pdf")
        plt.close(fig)
