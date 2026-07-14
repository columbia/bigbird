"""X/Y axis formatting helpers for the sweep and cause figures: tick thinning,
infinity/off tick labels, and the ``//`` axis break used to render an infinity
point past the finite range. Figure-only — none of this touches the CSVs.
"""

import math

import numpy as np
from matplotlib.ticker import FuncFormatter, MaxNLocator

from bigbird.plots.style import COLUMN_STYLES, FONT_AXIS_LABEL


def setup_axis(ax, xlabel, ylabel, min_x=0):
    ax.set_xlabel(xlabel, fontsize=FONT_AXIS_LABEL, labelpad=1)
    ax.set_ylabel(ylabel, fontsize=FONT_AXIS_LABEL, labelpad=2)
    ax.grid(True, linestyle="-", linewidth=0.3, alpha=0.2, color="#bbbbbb")
    ax.set_yticks([0.0, 0.2, 0.4, 0.6, 0.8, 1.0])
    ax.set_ylim(0, 1.0)
    ax.set_xlim(left=min_x)
    ax.spines["left"].set_bounds(0, 1.0)
    ax.xaxis.set_major_locator(MaxNLocator(nbins=8))


def filter_ticks(sorted_x):
    """Drop ticks that sit too close together, preferring grid-aligned points."""
    if len(sorted_x) <= 1:
        return sorted_x

    median_diff = np.median(np.diff(sorted_x))
    if median_diff == 0:
        return sorted_x

    # How well each point fits the median grid (higher == keep on conflict).
    supports = [
        sum(
            abs((abs(x - o) / median_diff) - round(abs(x - o) / median_diff)) < 0.15
            for o in sorted_x
        )
        for x in sorted_x
    ]

    indices = list(range(len(sorted_x)))
    changed = True
    while changed:
        changed = False
        k = 0
        while k < len(indices) - 1:
            a, b = indices[k], indices[k + 1]
            if sorted_x[b] - sorted_x[a] < 0.75 * median_diff:
                if supports[a] < supports[b] or (
                    supports[a] == supports[b]
                    and len(f"{sorted_x[a]:g}") > len(f"{sorted_x[b]:g}")
                ):
                    indices.pop(k)
                else:
                    indices.pop(k + 1)
                changed = True
            else:
                k += 1
    return [sorted_x[i] for i in indices]


def format_tick_labels(ticks, infinity_val=None):
    labels = []
    for x in ticks:
        if infinity_val is not None and math.isclose(x, infinity_val, abs_tol=1e-5):
            labels.append(r"$\infty$")
        elif isinstance(x, (int, np.integer)) or (
            isinstance(x, float) and x.is_integer()
        ):
            labels.append(str(int(x)))
        else:
            labels.append(f"{x:g}")
    return labels


def configure_x_ticks(ax, all_x, infinity_val, col, disabled_val, split_col=None):
    if not all_x:
        return
    sorted_x = sorted(all_x)

    if split_col in COLUMN_STYLES and "xlim_max" in COLUMN_STYLES[split_col]:
        cap = COLUMN_STYLES[split_col]["xlim_max"]
        if sorted_x[-1] <= cap:
            ax.set_xlim(right=cap)

    if len(sorted_x) > 20:
        return

    ticks = filter_ticks(sorted_x)
    if infinity_val is not None and infinity_val not in ticks:
        ticks = sorted(ticks + [infinity_val])
    ax.set_xticks(ticks)

    if col == "quota_count":
        ax.xaxis.set_major_formatter(
            FuncFormatter(
                lambda x, _: "off" if abs(x - disabled_val) < 1e-9 else f"{x:g}"
            )
        )
    else:
        ax.set_xticklabels(format_tick_labels(ticks, infinity_val=infinity_val))


def draw_axis_break(ax, finite_x, infinity_val):
    """Draw ``//`` marks on the x-spine between the last finite tick and infinity."""
    if not finite_x:
        return

    center = (finite_x[-1] + infinity_val) / 2
    gap = (infinity_val - finite_x[-1]) * 0.06
    dx = (infinity_val - finite_x[-1]) * 0.08
    y_lo, y_hi = ax.get_ylim()
    dy = (y_hi - y_lo) * 0.03

    ax.spines["bottom"].set_visible(False)
    to_axes = ax.transData + ax.transAxes.inverted()
    left_end = to_axes.transform((center - gap, 0))[0]
    right_start = to_axes.transform((center + gap, 0))[0]
    ax.plot([0, left_end], [0, 0], color="k", linewidth=0.6, transform=ax.transAxes, clip_on=False)
    ax.plot([right_start, 1], [0, 0], color="k", linewidth=0.6, transform=ax.transAxes, clip_on=False)

    for offset in (-gap, gap):
        cx = center + offset
        ax.plot(
            [cx - dx, cx + dx], [y_lo - dy, y_lo + dy], color="k", clip_on=False,
            linewidth=0.6, transform=ax.transData,
        )
