"""The paper's combined Fig. 6: three attack-resilience panels (domain-cap sweep,
sybil-domain sweep, site-popularity sweep) sharing one legend on the left.

Historically this figure was stitched together by hand (three separate plot PDFs
plus a hand-drawn shared legend). This module regenerates it deterministically
from the same per-panel CSVs the individual figures use, so `reproduce.sh` emits
the real Fig. 6 with no manual step.

Each panel is plotted directly from its `*_BY_attack_id_p50_95.csv` (the gated,
oracle-verified numbers), so the combined figure is guaranteed to agree with the
standalone panels cell-for-cell.
"""

import csv

import matplotlib.lines as mlines
import matplotlib.pyplot as plt

from bigbird.plots.axis import configure_x_ticks, setup_axis
from bigbird.plots.style import (
    LSTYLES,
    STYLE_VARIANTS,
    STYLES,
    build_plot_kwargs,
    legend_handle,
)

# CSV column headers name a series by either an attack-strategy label
# ("Random"/"Omniscient", from the attack_id split) or a system key
# ("PPA"/"CookieMonster", from the constant baselines). Resolve either to a
# style dict.
def _attack_style(v):
    # Start from the Big Bird base, apply the attack variant, then force the
    # marker edge/face to the series color so the marker matches its line (the
    # base leaves the edge Big-Bird-blue — the very mismatch the hand-made
    # legend existed to fix).
    s = {**STYLES["BigBird"], **v}
    s["markeredgecolor"] = v["color"]
    return s


_ATTACK_STYLE = {
    v["label"]: _attack_style(v) for v in STYLE_VARIANTS["attack_id"].values()
}


def _style_for(system):
    if system in _ATTACK_STYLE:
        return _ATTACK_STYLE[system]
    if system in STYLES:
        return STYLES[system]
    raise KeyError(f"no style for series {system!r}")


def _read_panel(path):
    """Return (x_col_name, rows) where rows is a list of (x_or_None, {series: val})."""
    with open(path) as f:
        reader = csv.reader(f)
        header = next(reader)
        x_col = header[0]
        # header cells look like "Random (p50)" -> (series, p-index)
        cols = []
        for h in header[1:]:
            name, _, ptag = h.rpartition(" (")
            p = ptag.rstrip(")")  # e.g. "p50"
            cols.append((name, p))
        rows = []
        for r in reader:
            x = float(r[0]) if r[0] != "" else None
            vals = {}
            for (series, p), cell in zip(cols, r[1:]):
                if cell != "":
                    vals[(series, p)] = float(cell)
            rows.append((x, vals))
    percentiles = sorted({p for _, p in cols}, reverse=True)  # ["p95","p50"]
    series = list(dict.fromkeys(s for s, _ in cols))
    return x_col, series, percentiles, rows


def _plot_panel(ax, path, x_col, xlabel):
    _, series_names, percentiles, rows = _read_panel(path)
    finite_x = sorted(x for x, _ in rows if x is not None)

    for series in series_names:
        style = _style_for(series)
        for p in percentiles:
            linestyle = LSTYLES[0] if p == "p50" else LSTYLES[1]
            pts = [(x, v[(series, p)]) for x, v in rows
                   if x is not None and (series, p) in v]
            const = [v[(series, p)] for x, v in rows
                     if x is None and (series, p) in v]
            if pts:  # a varying series
                xs, ys = zip(*sorted(pts))
                ax.plot(list(xs), list(ys), **build_plot_kwargs(style, linestyle))
            elif const:  # a constant baseline: flat line across the finite range
                ax.plot(finite_x, [const[0]] * len(finite_x),
                        **build_plot_kwargs(style, linestyle))

    pad = (min(_diffs(finite_x)) * 0.5) if len(finite_x) > 1 else 0.5
    setup_axis(ax, xlabel, "RMSRE", min_x=finite_x[0] - pad)
    configure_x_ticks(ax, set(finite_x), None, x_col, -999.0)


def _diffs(xs):
    return [b - a for a, b in zip(xs, xs[1:])]


def _shared_legend_handles(percentiles=("p95", "p50")):
    """Line-style rows (p95/p50) then the four series, in the paper's order."""
    handles = []
    for p in percentiles:
        ls = LSTYLES[0] if p == "p50" else LSTYLES[1]
        h = mlines.Line2D([], [], color="black", linestyle=ls, linewidth=0.8, label=p)
        if ls == ":":
            h.set_dashes((1, 1.5))
        handles.append(h)
    for series in ["Random", "Omniscient", "PPA", "CookieMonster"]:
        handles.append(legend_handle(_style_for(series)))
    return handles


def render_combined(panels, out_path, width=6.3, height=1.02):
    """panels: list of (csv_path, x_col, xlabel). Writes the combined Fig. 6.

    Sized so each panel is ~native (`FIG_WIDTH`≈1.63in), matching the standalone
    plots — so fonts and markers keep the same relative size the paper used."""
    fig = plt.figure(figsize=(width, height))
    gs = fig.add_gridspec(
        1, 1 + len(panels), width_ratios=[0.95] + [1] * len(panels),
        wspace=0.72, left=0.005, right=0.99, bottom=0.30, top=0.95,
    )

    ax_leg = fig.add_subplot(gs[0, 0])
    ax_leg.axis("off")
    ax_leg.legend(
        handles=_shared_legend_handles(), loc="center left",
        # Inset from the left edge so the legend sits next to the panels
        # rather than dangling at the figure's left margin.
        bbox_to_anchor=(0.25, 0.5), frameon=False, fontsize=5.5,
        handlelength=1.4, handletextpad=0.4, labelspacing=0.35, borderaxespad=0,
    )

    for i, (csv_path, x_col, xlabel) in enumerate(panels):
        ax = fig.add_subplot(gs[0, i + 1])
        _plot_panel(ax, csv_path, x_col, xlabel)

    fig.savefig(out_path)
    plt.close(fig)
    return out_path
