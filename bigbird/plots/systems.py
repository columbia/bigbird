"""Split a run's batches into the three plotted systems and resolve the
sentinel values that stand in for "filter disabled" (infinity / off).

A system is identified by which quota filters are active, encoded in the config
columns (``-1`` / null == disabled):

  * Big Bird       : per-querier cap on, plus an impression quota OR a domain cap
  * Attribution w/ global  (PPA) : per-querier cap on, no impression quota / domain cap
  * Attribution w/o global (CM)  : no per-querier cap, no impression quota / domain cap
"""

from dataclasses import dataclass

import polars as pl

# Columns that never make sense as a "split-by" dimension (per-batch measures,
# ids, and the metrics themselves). Everything else with >1 distinct value in a
# BB-only sweep becomes an auto-detected split.
IGNORE_SPLIT_COLS = {
    "rmsre",
    "fraction_reports",
    "item_number",
    "timestamp_min",
    "querier_id",
    "start_epoch",
    "end_epoch",
    "requested_epsilon",
    "noise_scale",
    "dropped_epochs",
    "dropped_nc",
    "dropped_c",
    "dropped_qcount",
    "relative_bias",
    "fraction_dropped_nc",
    "fraction_dropped_c",
    "fraction_dropped_qcount",
}


def split_df_by_system(df: pl.DataFrame):
    """Return (big_bird, ppa, cookie_monster) subframes. Every row must classify."""

    def disabled(col):
        return (pl.col(col) == -1) | (pl.col(col).is_null())

    def enabled(col):
        return (pl.col(col) != -1) & (pl.col(col).is_not_null())

    mask_bb = enabled("eps_c") & (enabled("eps_qimp") | enabled("quota_count"))
    mask_ppa = enabled("eps_c") & disabled("eps_qimp") & disabled("quota_count")
    mask_cm = disabled("eps_c") & disabled("eps_qimp") & disabled("quota_count")

    unclassified = df.filter(~(mask_bb | mask_ppa | mask_cm))
    if unclassified.height > 0:
        raise ValueError(
            "rows could not be classified into systems:\n"
            f"{unclassified.select('eps_c', 'eps_qimp', 'quota_count')}"
        )

    return df.filter(mask_bb), df.filter(mask_ppa), df.filter(mask_cm)


def find_splitting_cols(df: pl.DataFrame, x_col: str) -> list[str]:
    """Config columns (besides x_col) that take more than one value -> splits."""
    numeric_or_str = (pl.Float64, pl.Int32, pl.Int64, pl.Utf8, pl.Boolean)
    return [
        col
        for col in df.columns
        if col != x_col
        and col not in IGNORE_SPLIT_COLS
        and df[col].dtype in numeric_or_str
        and df[col].n_unique() > 1
    ]


# ------------------------------------------------------------------------------
# Sentinel (-1 == infinity / disabled) handling for the x-axis parameter.
# eps_qimp: -1 means the impression quota is off (infinity), drawn past the last
#   finite tick. quota_count: -1 means the domain cap is off, drawn as "off".
# ------------------------------------------------------------------------------


@dataclass(frozen=True)
class PreparedData:
    bb: pl.DataFrame
    ppa: pl.DataFrame
    cm: pl.DataFrame
    infinity_val: float | None
    disabled_val: float


def _infinity_val(valid_vals):
    vals = sorted(v for v in valid_vals if v is not None)
    if not vals:
        return None
    step = vals[-1] - vals[-2] if len(vals) > 1 else (vals[0] or 1.0)
    return vals[-1] + 2 * step


def _disabled_val(valid_vals):
    vals = sorted(v for v in valid_vals if v is not None)
    if not vals:
        return 10
    step = vals[-1] - vals[-2] if len(vals) > 1 else (vals[0] or 10)
    return vals[-1] + step


def _replace_neg1(df, col, replacement):
    return df.with_columns(
        pl.when(pl.col(col) == -1)
        .then(replacement)
        .otherwise(pl.col(col))
        .alias(col)
    )


def _finite_unique(df, col):
    return (
        df.filter((pl.col(col) != -1) & pl.col(col).is_not_null())[col]
        .unique()
        .to_list()
    )


def replace_special_values(bb, ppa, cm, col) -> PreparedData:
    """Map the -1 sentinel to a plotted x-position (infinity for eps_qimp,
    'off' for quota_count) across all three system frames."""
    infinity_val = None
    disabled_val = 0

    if col == "eps_qimp":
        infinity_val = _infinity_val(_finite_unique(bb, col))
        if infinity_val is not None:
            bb = _replace_neg1(bb, col, infinity_val)

    if col == "quota_count":
        all_finite = (
            set(_finite_unique(bb, col))
            | set(_finite_unique(ppa, col))
            | set(_finite_unique(cm, col))
        )
        disabled_val = _disabled_val(list(all_finite))
        bb = _replace_neg1(bb, col, disabled_val)
        ppa = _replace_neg1(ppa, col, disabled_val)
        cm = _replace_neg1(cm, col, disabled_val)

    fill = disabled_val if col == "quota_count" else 0
    bb = bb.with_columns(pl.col(col).fill_null(fill))
    ppa = ppa.with_columns(pl.col(col).fill_null(fill))
    cm = cm.with_columns(pl.col(col).fill_null(fill))

    return PreparedData(bb, ppa, cm, infinity_val, disabled_val)


def replace_special_values_single(df, col):
    """Single-frame variant used by the error-cause chart. Returns
    (df, infinity_val, disabled_val)."""
    infinity_val = None
    disabled_val = 0

    if col == "eps_qimp":
        infinity_val = _infinity_val(_finite_unique(df, col))
        if infinity_val is not None:
            df = _replace_neg1(df, col, infinity_val)

    if col == "quota_count":
        disabled_val = _disabled_val(_finite_unique(df, col))
        df = _replace_neg1(df, col, disabled_val)

    return df, infinity_val, disabled_val
