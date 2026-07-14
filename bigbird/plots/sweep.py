"""Parameter-sweep figures: RMSRE (p50/p95 quantiles) vs. a swept parameter.

Produces both the paper figure and the verification CSV. The CSV is the gated
artifact, so the aggregation (polars ``quantile`` with its default "nearest"
interpolation) and the per-system/per-split column layout are load-bearing.
"""

import math

import matplotlib.lines as mlines
import matplotlib.pyplot as plt
import numpy as np
import polars as pl

from bigbird.plots.axis import configure_x_ticks, draw_axis_break, setup_axis
from bigbird.plots.style import (
    COLUMN_STYLES,
    FIG_HEIGHT,
    FIG_WIDTH,
    FONT_LEGEND,
    FONT_LEGEND_SMALL,
    LSTYLES,
    STYLES,
    STYLE_VARIANTS,
    auto_styles,
    build_plot_kwargs,
    legend_handle,
)
from bigbird.plots.systems import (
    find_splitting_cols,
    replace_special_values,
    split_df_by_system,
)


def plot_system(ax, df, x_col, style, percentiles):
    """Plot one system/series as p50/p95 quantile lines over the swept x.

    The returned summary frame's ``system`` column uses the style *label* — this
    label becomes the CSV column header, so it is load-bearing."""
    aggs = [pl.col("rmsre").quantile(p / 100).alias(f"p{p}") for p in percentiles]
    agg = df.group_by(x_col).agg(aggs).sort(x_col)

    x = agg[x_col].to_numpy()
    for i, p in enumerate(percentiles):
        ax.plot(x, agg[f"p{p}"].to_numpy(), **build_plot_kwargs(style, LSTYLES[i]))

    summary = agg.with_columns(pl.lit(style["label"]).alias("system"))
    return legend_handle(style), summary


def plot_constant_system(ax, df, x_ref, style_key, percentiles):
    """Plot a system whose value does not vary with x (PPA/CM on BB-only params)
    as a flat line at its own quantile.

    Load-bearing quirk preserved from the original: the summary frame's x column
    is hard-named ``eps_qimp`` (``x_ref`` is a numpy array, so it has no name).
    When the swept parameter *is* eps_qimp the constant value is broadcast across
    every x; for any other parameter these rows land under a null x key in the
    pivoted CSV (a single "constant" row). The ``system`` column uses the style
    *key* (e.g. "PPA"), not its label. Both are exactly what the golden CSVs
    encode."""
    style = STYLES[style_key]
    values = [df["rmsre"].quantile(p / 100) for p in percentiles]

    summary = {"system": [style_key] * len(x_ref), "eps_qimp": x_ref}
    for i, (p, val) in enumerate(zip(percentiles, values)):
        ax.plot(x_ref, [val] * len(x_ref), **build_plot_kwargs(style, LSTYLES[i]))
        summary[f"p{p}"] = [val] * len(x_ref)

    return legend_handle(style), pl.DataFrame(summary)


def make_readable_df(summaries, index_col, percentiles):
    """Pivot per-system summaries into one wide CSV: index + '<system> (pXX)'."""
    if not summaries:
        return None

    full = pl.concat(summaries, how="diagonal")
    p_cols = [f"p{p}" for p in percentiles]
    pivoted = full.pivot(
        index=index_col, on="system", values=p_cols, aggregate_function="first"
    ).sort(index_col)

    selects = [pl.col(index_col)]
    for system in sorted(full["system"].unique()):
        for p, p_col in zip(percentiles, p_cols):
            # Polars names pivoted columns 'pXX_<system>', collapsing to just
            # '<system>' when a single value column is pivoted.
            name = f"{p_col}_{system}"
            if name not in pivoted.columns and system in pivoted.columns:
                name = system
            if name in pivoted.columns:
                selects.append(pl.col(name).round(6).alias(f"{system} ({p_col})"))
    return pivoted.select(selects)


def gen_param_sweep_figs(
    df, percentiles, col, xlabel, exclude_split_vals=None, show_legend=True,
):
    """Yield (fig, csv_df, split_col) per sweep. Multiple splits (e.g. attack_id
    plus auto-detected config columns) yield multiple figures."""
    if df.filter(pl.col(col) != -1).select(col).unique().height <= 1:
        return

    bb, ppa, cm = split_df_by_system(df)
    prep = replace_special_values(bb, ppa, cm, col)
    bb, ppa, cm = prep.bb, prep.ppa, prep.cm

    for split_col in _split_columns(bb, ppa, cm, col):
        fig, ax = _new_axes(show_legend)
        handles, summaries, x_ref = [], [], None

        # --- Big Bird (varies with x, optionally split into several series) ---
        if split_col is not None:
            handles, summaries, x_ref = _plot_bb_split(
                ax, bb, col, percentiles, split_col, exclude_split_vals
            )
        elif bb.height > 0 and bb.group_by(col).len().height >= 2:
            x_ref = (
                bb.sort(col).select(col).unique(maintain_order=True)[col].to_numpy()
            )
            h, d = plot_system(ax, bb, col, STYLES["BigBird"], percentiles)
            handles.append(h)
            summaries.append(d)

        # --- PPA & CM: constant line if single-valued, else a normal sweep ---
        for sys_df, key in [(ppa, "PPA"), (cm, "CookieMonster")]:
            if sys_df.height == 0:
                continue
            varies = (
                sys_df.filter((pl.col(col) != -1) & pl.col(col).is_not_null())
                .select(col)
                .n_unique()
                > 1
            )
            if varies:
                h, d = plot_system(ax, sys_df, col, STYLES[key], percentiles)
                handles.append(h)
                summaries.append(d)
            elif x_ref is not None:
                h, d = plot_constant_system(ax, sys_df, x_ref, key, percentiles)
                handles.append(h)
                summaries.append(d)

        _draw_legend(fig, handles, percentiles, split_col, show_legend)
        _format_axes(ax, xlabel, col, x_ref, summaries, prep, split_col)

        yield fig, make_readable_df(summaries, col, percentiles), split_col


# ------------------------------------------------------------------------------
# Split selection + BB-split plotting
# ------------------------------------------------------------------------------


def _split_columns(bb, ppa, cm, col):
    """Which extra dimension(s) to split Big Bird by. Returns [None] for a plain
    per-system sweep, else one entry per split (each produces its own figure)."""
    only_bb = ppa.height == 0 and cm.height == 0 and bb.height > 0
    if only_bb:
        splits = find_splitting_cols(bb, col)
        return splits if splits else [None]
    if bb.height > 0 and "attack_id" in bb.columns and bb["attack_id"].n_unique() > 1:
        return ["attack_id"]
    return [None]


def _plot_bb_split(ax, bb, col, percentiles, split_col, exclude_split_vals):
    handles, summaries = [], []

    split_vals = _sorted_split_vals(bb, split_col, exclude_split_vals)
    if len(split_vals) > 10:
        return handles, summaries, None

    for val, style in zip(split_vals, _split_styles(split_col, split_vals)):
        subset = (
            bb.filter(pl.col(split_col).is_null())
            if val is None
            else bb.filter(pl.col(split_col) == val)
        )
        if subset.height > 0:
            h, d = plot_system(ax, subset, col, style, percentiles)
            handles.append(h)
            summaries.append(d)

    x_ref = bb.sort(col).select(col).unique(maintain_order=True)[col].to_numpy()
    return handles, summaries, x_ref


def _sorted_split_vals(bb, split_col, exclude_split_vals):
    vals = bb[split_col].unique().to_list()
    try:
        vals = sorted(vals)
    except TypeError:
        vals = sorted(vals, key=lambda x: (x is None, x))

    if exclude_split_vals and split_col in exclude_split_vals:
        vals = [v for v in vals if v not in exclude_split_vals[split_col]]

    # Draw larger markers first, keep attack_id 0 (No Attack) drawn last/on top.
    if split_col == "attack_id":
        has_zero = 0 in vals
        vals = [v for v in vals if v != 0]
        vals.sort(
            key=lambda a: STYLE_VARIANTS["attack_id"].get(a, {}).get("markersize", 0),
            reverse=True,
        )
        if has_zero:
            vals.append(0)
    return vals


def _split_styles(split_col, split_vals):
    """Per-series style dicts. attack_id uses the fixed attack palette; any other
    (auto-detected) split gets generated styles labelled by the raw value."""
    if split_col in STYLE_VARIANTS:
        variants = STYLE_VARIANTS[split_col]
        styles = []
        for v in split_vals:
            style = STYLES["BigBird"].copy()
            if v in variants:
                overrides = variants[v]
                style.update(overrides)
                if "color" in overrides:
                    style.setdefault("markeredgecolor", overrides["color"])
                    style.setdefault("markerfacecolor", overrides["color"])
            else:
                style["label"] = str(v)
            styles.append(style)
        return styles

    styles = auto_styles([str(v) for v in split_vals])
    if split_col in COLUMN_STYLES:
        for style in styles:
            style.update(COLUMN_STYLES[split_col])
    return styles


# ------------------------------------------------------------------------------
# Figure chrome (legend, axes) — does not affect the CSV.
# ------------------------------------------------------------------------------


def _new_axes(show_legend):
    if show_legend:
        fig_h, top, bottom = FIG_HEIGHT, 0.76, 0.22
    else:
        top = 0.95
        fig_h = (0.54 * FIG_HEIGHT) / (top - 0.22)
        bottom = 0.22 * FIG_HEIGHT / fig_h
    fig, ax = plt.subplots(figsize=(FIG_WIDTH, fig_h))
    fig.subplots_adjust(right=0.97, left=0.22, bottom=bottom, top=top)
    return fig, ax


def _draw_legend(fig, handles, percentiles, split_col, show_legend):
    if not (handles and show_legend):
        return

    style_handles = []
    if len(percentiles) > 1:
        for p in sorted(percentiles, reverse=True):
            lstyle = LSTYLES[percentiles.index(p)]
            h = mlines.Line2D(
                [], [], color="black", linestyle=lstyle, linewidth=0.8, label=f"p{p}"
            )
            if lstyle == ":":
                h.set_dashes((1, 1.5))
            style_handles.append(h)

    empty = mlines.Line2D([], [], linestyle="None", label="")
    if split_col is None and style_handles:
        rows = max(len(style_handles), len(handles), 1)
        col1 = style_handles + [empty] * (rows - len(style_handles))
        col2 = handles + [empty] * (rows - len(handles))
        final, fontsize = col1 + col2, FONT_LEGEND
    else:
        items = style_handles + handles
        rows = (len(items) + 1) // 2
        col1, col2 = items[:rows], items[rows:]
        col2 += [empty] * (rows - len(col2))
        final = col1 + col2
        fontsize = FONT_LEGEND_SMALL if len(items) > 4 else FONT_LEGEND

    fig.legend(
        handles=final,
        loc="upper right",
        bbox_to_anchor=(0.97, 0.99),
        ncol=2,
        frameon=False,
        fontsize=fontsize,
        borderaxespad=0,
        columnspacing=0.8,
        handletextpad=0.3,
        labelspacing=0.2,
    )


def _format_axes(ax, xlabel, col, x_ref, summaries, prep, split_col):
    all_x = set(x_ref) if x_ref is not None else set()
    for d in summaries:
        if col in d.columns:
            all_x.update(d[col].to_list())

    left = 0
    if all_x:
        sorted_x = sorted(all_x)
        if len(sorted_x) > 1:
            padding = np.min(np.diff(sorted_x)) * 0.5
        else:
            padding = sorted_x[0] * 0.5 if sorted_x[0] > 0 else 0.5
        left = sorted_x[0] - padding

    setup_axis(ax, xlabel, "RMSRE", min_x=left)
    configure_x_ticks(ax, all_x, prep.infinity_val, col, prep.disabled_val, split_col)

    if col == "eps_qimp" and prep.infinity_val is not None and all_x:
        finite_x = sorted(
            v for v in all_x if not math.isclose(v, prep.infinity_val, abs_tol=1e-5)
        )
        draw_axis_break(ax, finite_x, prep.infinity_val)
