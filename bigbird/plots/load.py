"""Load Rust runtime logs (gzipped JSON) into a flat Polars DataFrame.

One JSON file per experiment run; one row per querier batch. Each row carries
the batch's aggregation vectors plus the run's config flattened onto every row
(so downstream code can filter/split on config values). The read-side metric
(rmsre / relative_bias) is attached here.
"""

import gzip
import json
from pathlib import Path

import numpy as np
import polars as pl

from bigbird.metrics import rmsre_tau


def load_dir(logs_dir) -> pl.DataFrame:
    """Load and column-align every ``*.json.gz`` log under ``logs_dir``.

    Filesystem glob order (not sorted) is preserved on purpose: the error-cause
    chart averages per-batch fractions, and float summation order is sensitive
    to the row order produced by this concat. Sorting would shift a few values
    by 1 ULP and diverge from the golden CSVs. (The RMSRE sweeps use quantiles
    and are order-independent.)"""
    dfs = [load_log(p) for p in Path(logs_dir).glob("*.json.gz")]
    return concat_aligned(dfs)


def load_log(path) -> pl.DataFrame:
    with gzip.open(path, "rt", encoding="utf-8") as f:
        raw = json.load(f)
    return _batches_with_config(raw)


def concat_aligned(dfs: list[pl.DataFrame]) -> pl.DataFrame:
    """Vertically concat DataFrames, filling absent columns with null."""
    columns = sorted({c for df in dfs for c in df.columns})
    aligned = [
        df.with_columns(
            [pl.lit(None).alias(c) for c in columns if c not in df.columns]
        ).select(columns)
        for df in dfs
    ]
    return pl.concat(aligned, how="vertical_relaxed")


def _batches_with_config(raw: dict) -> pl.DataFrame:
    df = _batches(raw)

    # Flatten the run config into one value-per-column, broadcast over all rows.
    config = dict(raw["runtime_config"])
    config.update(config.pop("common_querier_config"))
    config.update(raw["workload_config"])
    for name, cap in raw["capacities"].items():
        # -1 is the "filter disabled" (infinity) sentinel used by split logic.
        config[f"capacity_{name}"] = cap if cap is not None else -1
    if raw.get("attack_stats"):
        for name, value in raw["attack_stats"].items():
            config[f"atk_stat_{name}"] = value
    for unused in ("log_path", "save_detailed_logs"):
        config.pop(unused, None)

    return df.with_columns([pl.lit(v).alias(k) for k, v in config.items()])


def _batches(raw: dict) -> pl.DataFrame:
    tau_per_report = raw["runtime_config"]["common_querier_config"][
        "tau_per_report"
    ]

    def metrics(row):
        tau = row["batch_size"] * tau_per_report
        filtered = np.array(row["filtered_aggregation"])
        unfiltered = np.array(row["unfiltered_aggregation"])
        return {
            "relative_bias": rmsre_tau(filtered, unfiltered, 0, tau),
            "rmsre": rmsre_tau(filtered, unfiltered, row["noise_scale"], tau),
        }

    return (
        pl.from_dicts(raw["query_results"], strict=False)
        .with_columns(
            _metrics=pl.struct(
                "filtered_aggregation",
                "unfiltered_aggregation",
                "batch_size",
                "noise_scale",
            ).map_elements(metrics, return_dtype=pl.Struct)
        )
        .unnest("_metrics")
        .with_columns(
            fraction_dropped_nc=pl.col("dropped_nc") / pl.col("batch_size"),
            fraction_dropped_c=pl.col("dropped_c") / pl.col("batch_size"),
            fraction_dropped_qconv=pl.col("dropped_qconv") / pl.col("batch_size"),
            fraction_dropped_qimp=pl.col("dropped_qimp") / pl.col("batch_size"),
            fraction_dropped_qcount=pl.col("dropped_qcount")
            / pl.col("batch_size"),
        )
    )
