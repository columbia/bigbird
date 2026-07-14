"""Stacked-area chart of *why* reports were dropped, as a fraction of the batch.

Each batch reports how many reports were dropped for each budget-exhaustion
cause; this turns those into per-x fractions and stacks them. Only Big Bird
produces a meaningful chart here (PPA/CM have too few x points), so callers skip
the None result.
"""

import math

import matplotlib.patches as mpatches
import matplotlib.pyplot as plt
import polars as pl
from matplotlib.ticker import FuncFormatter

from bigbird.plots.axis import draw_axis_break, filter_ticks, format_tick_labels
from bigbird.plots.style import (
    FIG_HEIGHT,
    FIG_WIDTH,
    FONT_AXIS_LABEL,
    FONT_LEGEND,
    STACK_LAYERS,
)
from bigbird.plots.systems import replace_special_values_single

# The dropped-report columns and the short cause name each maps to. The order
# here is load-bearing: it is the column order the melt stacks by, and polars'
# grouped mean is float-summation-order sensitive, so this order must match the
# golden pipeline (reordering shifts a few cause values by 1 ULP).
_CAUSE_COLS = {
    "fraction_dropped_qimp": "qimp",
    "fraction_dropped_qconv": "qconv",
    "fraction_dropped_nc": "nc",
    "fraction_dropped_c": "c",
    "fraction_dropped_qcount": "qcount",
}

# Deterministic CSV column order for the cause columns (bottom-to-top of stack).
_CAUSE_ORDER = ["qcount", "qimp", "qconv", "nc", "c"]


def get_oob_causes(df: pl.DataFrame) -> pl.DataFrame:
    """Melt the per-batch fraction_dropped_* columns into (metadata, oob_cause,
    fraction_reports) long form."""
    cols = [c for c in _CAUSE_COLS if c in df.columns]
    index_cols = [c for c in df.columns if c not in cols]
    return (
        df.select(
            pl.all().exclude(cols),
            *[pl.col(c).fill_null(0.0) for c in cols],
        )
        .unpivot(
            index=index_cols,
            on=cols,
            variable_name="raw_col",
            value_name="fraction_reports",
        )
        .with_columns(oob_cause=pl.col("raw_col").replace(_CAUSE_COLS))
        .drop("raw_col")
    )


def fig_error_causes(df, x_col, x_label, show_all_legend=True, show_legend=True):
    """Return (fig, csv_df) or (None, None) when there is nothing to plot.

    The CSV column order is fixed to [x_col] + _CAUSE_ORDER for reproducibility."""
    if df.height == 0:
        return None, None

    df, infinity_val, disabled_val = replace_special_values_single(df, x_col)

    pivoted = (
        df.group_by([x_col, "oob_cause"])
        .agg(pl.col("fraction_reports").mean())
        .pivot(on="oob_cause", index=x_col, values="fraction_reports")
        .sort(x_col)
        .fill_null(0.0)
    )
    if pivoted.height <= 2:
        return None, None

    # Stabilize column order (the pivot otherwise leaks cause iteration order).
    ordered = [x_col] + [c for c in _CAUSE_ORDER if c in pivoted.columns]
    csv_df = pivoted.select(ordered)

    fig = _render(
        pivoted, x_col, x_label, infinity_val, disabled_val,
        show_all_legend, show_legend,
    )
    return fig, csv_df


def _render(
    pivoted, x_col, x_label, infinity_val, disabled_val, show_all_legend, show_legend
):
    if show_legend:
        fig_h, top, bottom = FIG_HEIGHT, 0.82, 0.22
    else:
        top = 0.95
        fig_h = (0.60 * FIG_HEIGHT) / (top - 0.22)
        bottom = 0.22 * FIG_HEIGHT / fig_h
    fig, ax = plt.subplots(figsize=(FIG_WIDTH, fig_h))
    fig.subplots_adjust(left=0.27, bottom=bottom, right=0.97, top=top)

    x_vals = pivoted[x_col].to_list()
    layers = STACK_LAYERS
    if not show_all_legend:
        layers = [layer for layer in STACK_LAYERS if layer[0] in ("c", "nc", "qimp")]

    y_stack, colors, hatches, labels = [], [], [], []
    for key, label, color, hatch in layers:
        y_stack.append(
            pivoted[key].to_list() if key in pivoted.columns else [0.0] * len(x_vals)
        )
        colors.append(color)
        hatches.append(hatch)
        labels.append(label)

    if infinity_val is not None:
        stacks = _stack_with_infinity(
            ax, x_vals, y_stack, colors, hatches, infinity_val
        )
    else:
        stacks = ax.stackplot(x_vals, y_stack, colors=colors)

    for stack, hatch, color in zip(stacks, hatches, colors):
        stack.set_facecolor(color)
        stack.set_hatch(hatch)
        stack.set_edgecolor("black")
        stack.set_linewidth(0.15)

    ax.set_ylim(0, 1)
    ax.yaxis.set_major_formatter(FuncFormatter(lambda y, _: f"{y:.0%}"))
    ax.set_ylabel("% reports impacted", fontsize=FONT_AXIS_LABEL, labelpad=1)
    ax.set_xlabel(x_label, fontsize=FONT_AXIS_LABEL, labelpad=1)
    ax.set_xlim(0, max(x_vals))

    _configure_ticks(ax, x_vals, x_col, infinity_val, disabled_val)
    ax.grid(True, axis="y", linestyle="-", linewidth=0.3, alpha=0.3, color="black", zorder=0)

    if show_legend:
        handles = [
            mpatches.Patch(
                facecolor=color,
                edgecolor="black" if hatch else color,
                hatch=hatch,
                label=label,
                linewidth=0.6 if hatch else 0,
            )
            for label, color, hatch in reversed(list(zip(labels, colors, hatches)))
        ]
        ax.legend(
            handles=handles, loc="upper left", bbox_to_anchor=(0.0, 1.28),
            frameon=True, facecolor="white", edgecolor="none", framealpha=0.8,
            fontsize=FONT_LEGEND, handlelength=1.5, handleheight=0.8,
            labelspacing=0.2, ncol=1, columnspacing=0.5,
        )

    if infinity_val is not None:
        finite_x = sorted(
            v for v in set(x_vals) if not math.isclose(v, infinity_val, abs_tol=1e-5)
        )
        draw_axis_break(ax, finite_x, infinity_val)

    return fig


def _stack_with_infinity(ax, x_vals, y_stack, colors, hatches, infinity_val):
    finite_idx = [
        i for i, x in enumerate(x_vals)
        if not math.isclose(x, infinity_val, abs_tol=1e-5)
    ]
    inf_idx = [
        i for i, x in enumerate(x_vals)
        if math.isclose(x, infinity_val, abs_tol=1e-5)
    ]

    finite_x = [x_vals[i] for i in finite_idx]
    finite_y = [[layer[i] for i in finite_idx] for layer in y_stack]

    # Extend the area 25% of the way toward the infinity marker.
    finite_x.append(finite_x[-1] + 0.25 * (infinity_val - finite_x[-1]))
    for layer in finite_y:
        layer.append(layer[-1])
    stacks = ax.stackplot(finite_x, finite_y, colors=colors)

    if inf_idx:
        bar_width = (infinity_val - finite_x[-1]) * 0.5
        bottom = 0.0
        for idx in range(len(y_stack)):
            val = y_stack[idx][inf_idx[0]]
            ax.bar(
                infinity_val, val, bottom=bottom, width=bar_width, color=colors[idx],
                hatch=hatches[idx],
                edgecolor="black" if hatches[idx] else colors[idx],
                linewidth=0.5 if hatches[idx] else 0,
            )
            bottom += val
    return stacks


def _configure_ticks(ax, x_vals, x_col, infinity_val, disabled_val):
    sorted_x = sorted(set(x_vals))
    if infinity_val is not None and len(sorted_x) <= 20:
        ticks = filter_ticks(sorted_x)
        if infinity_val not in ticks:
            ticks = sorted(ticks + [infinity_val])
        ax.set_xticks(ticks)
        ax.set_xticklabels(format_tick_labels(ticks, infinity_val=infinity_val))
        ax.set_xlim(0, max(ticks))

    if x_col == "quota_count":
        ax.xaxis.set_major_formatter(
            FuncFormatter(
                lambda x, _: "off" if abs(x - disabled_val) < 1e-9 else f"{x:g}"
            )
        )
        if len(sorted_x) <= 20:
            ax.set_xticks(filter_ticks(sorted_x))
