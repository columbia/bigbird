"""Matplotlib styling for the paper figures: one place for sizes, fonts, and the
two palettes (systems and attacks).

Note: a style dict's ``label`` is drawn in the legend AND written as the CSV
column header for that series, so labels are part of the gated output.
"""

import matplotlib.lines as mlines
import matplotlib.pyplot as plt

from bigbird.common import SYSNAME

# Native paper sizing (acmart sigplan two-column): 0.49 * 3.333" column width.
# Created at native size so 1 pt on screen == 1 pt on paper.
FIG_WIDTH = 1.633
FIG_HEIGHT = 1.30

FONT_AXIS_LABEL = 7
FONT_TICK = 6
FONT_LEGEND = 5.5
FONT_LEGEND_SMALL = 5

C_BIGBIRD = "#1f77ff"
C_PPA = "#ff7f0e"
C_COOKIE = "#2ca02c"

# The three plotted systems. Keys ("PPA"/"CookieMonster") are internal; labels
# are the de-anonymized paper names.
STYLES = {
    "BigBird": {
        "label": SYSNAME,
        "color": C_BIGBIRD,
        "marker": "s",
        "markersize": 4,
        "markeredgecolor": C_BIGBIRD,
        "markerfacecolor": "none",
        "markeredgewidth": 0.6,
        "linewidth": 0.8,
    },
    "PPA": {
        "label": "Attribution w/ global",
        "color": C_PPA,
        "marker": "o",
        "markersize": 4,
        "markeredgecolor": C_PPA,
        "markerfacecolor": "none",
        "markeredgewidth": 0.6,
        "linewidth": 0.8,
    },
    "CookieMonster": {
        "label": "Attribution w/o global",
        "color": C_COOKIE,
        "marker": "x",
        "markersize": 4,
        "markeredgecolor": C_COOKIE,
        "markerfacecolor": C_COOKIE,
        "markeredgewidth": 0.6,
        "linewidth": 0.8,
    },
}

# The four attacks, used when Big Bird is split by attack_id (Fig 6/7).
STYLE_VARIANTS = {
    "attack_id": {
        0: {"label": "No Attack", "color": "#4e79a7", "marker": "x", "markersize": 4, "markeredgewidth": 0.6},
        1: {"label": "Indiscriminate", "color": "#9467bd", "marker": "^", "markerfacecolor": "none", "markersize": 4, "markeredgewidth": 0.6},
        2: {"label": "Random", "color": "#8c564b", "marker": "s", "markerfacecolor": "none", "markersize": 4, "markeredgewidth": 0.6},
        3: {"label": "Omniscient", "color": "#17becf", "marker": "o", "markerfacecolor": "none", "markersize": 4, "markeredgewidth": 0.6},
    }
}

# Per-split figure tweaks (num_sybils has many series -> smaller markers).
COLUMN_STYLES = {
    "num_sybils": {
        "markersize": 3,
        "linewidth": 0.7,
        "markeredgewidth": 0.5,
        "alpha": 0.7,
        "xlim_max": 1.0,
    }
}

# Index 0 = p50 (solid), index 1 = p95 (dotted).
LSTYLES = ["-", ":"]

# Error-cause stack, bottom to top: (cause key, legend label, color, hatch).
STACK_LAYERS = [
    ("qcount", "Domain cap", "white", "..."),
    ("qimp", r"Impression quota ($\epsilon_{imp}$)", "#f0f0f0", "////"),
    ("qconv", r"Conv. quota ($\epsilon_{conv}$)", "#d0d0d0", "xx"),
    ("nc", "Per-querier", "#a0a0a0", None),
    ("c", "Global", "#404040", None),
]


def apply_rcparams():
    plt.rcParams.update(
        {
            "font.size": FONT_TICK,
            "font.family": "sans-serif",
            "font.sans-serif": ["DejaVu Sans", "Arial", "Helvetica", "sans-serif"],
            "font.weight": "normal",
            "axes.linewidth": 0.4,
            "axes.spines.top": False,
            "axes.spines.right": False,
            "xtick.major.width": 0.4,
            "ytick.major.width": 0.4,
            "xtick.major.size": 2,
            "ytick.major.size": 2,
            "xtick.major.pad": 2,
            "ytick.major.pad": 2,
            "legend.frameon": False,
            "legend.fontsize": FONT_LEGEND,
            "legend.handlelength": 1.2,
            "legend.handletextpad": 0.3,
        }
    )


apply_rcparams()


def build_plot_kwargs(style, linestyle, zorder=5):
    """Matplotlib line kwargs from a style dict. p95 (dotted) lines suppress
    markers via a fine dash pattern per reviewer feedback."""
    kwargs = {
        "color": style["color"],
        "linestyle": linestyle,
        "linewidth": style["linewidth"],
        "clip_on": False,
        "zorder": zorder,
        "marker": style["marker"],
        "markersize": style["markersize"],
        "markeredgecolor": style["markeredgecolor"],
        "markerfacecolor": style.get("markerfacecolor", "none"),
        "markeredgewidth": style.get("markeredgewidth", 0.6),
    }
    if linestyle == ":":
        kwargs["dashes"] = (1, 1.5)
    if "alpha" in style:
        kwargs["alpha"] = style["alpha"]
    return kwargs


def legend_handle(style):
    """A marker-only legend handle for a series."""
    return mlines.Line2D(
        [], [], color=style["color"], marker=style["marker"], linestyle="None",
        markersize=style["markersize"], markeredgecolor=style["markeredgecolor"],
        markerfacecolor=style.get("markerfacecolor", "none"),
        markeredgewidth=style.get("markeredgewidth", 0.6), label=style["label"],
    )


def auto_styles(labels):
    """Generated styles for an auto-detected split (color+marker per series).
    The label is the raw split value, which becomes the CSV column header."""
    colors = ["#9467bd", "#17becf", "#8c564b", "#e377c2", "#bcbd22", "#7f7f7f"]
    markers = ["p", "*", "h", "d", "8", "+"]
    styles = []
    for i, label in enumerate(labels):
        style = STYLES["BigBird"].copy()
        color = colors[i % len(colors)]
        style.update(
            label=str(label), color=color, markeredgecolor=color,
            markerfacecolor=color, marker=markers[i % len(markers)],
        )
        styles.append(style)
    return styles
