import marimo

__generated_with = "0.12.4"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import plotly.express as px
    import polars as pl

    from bigbird.plots.load import load_dir as load_query_result_dir
    from bigbird.common import REPO_ROOT
    return REPO_ROOT, load_query_result_dir, mo, pl, px


@app.cell
def _(REPO_ROOT, load_query_result_dir):
    # df = load_query_result_dir(REPO_ROOT.joinpath("logs/runtime_outputs_04_05_19_04_19"))
    df = load_query_result_dir(REPO_ROOT.joinpath("logs/runtime_outputs_04_06_09_59_40/"))

    df
    return (df,)


@app.cell
def _():
    return


@app.cell
def _(mo):
    dropdown = mo.ui.dropdown(options=[0.01, 0.025, 0.05, 0.075, 0.1, 0.15, 0.2, 0.25, 0.3], value=0.05, label="Target RMSRE")
    dropdown
    return (dropdown,)


@app.cell
def _(df, dropdown, pl):
    filtered_df = df.filter(pl.col("rmsre_target") == dropdown.value).sort("rmsre")
    filtered_df.select(["timestamp_min", "requested_epsilon", "batch_size", "rmsre", "oob_frac", "nc_frac", "c_frac", "qconvimp_frac", "qconv_frac", "qimp_frac"]).describe()
    return (filtered_df,)


@app.cell
def _(filtered_df, px):
    px.line(
        filtered_df,
        y="rmsre",
        range_y=[0,1])
    return


@app.cell
def _(df, px):
    px.box(
        df,
        y="rmsre",
        x="rmsre_target",
        range_y=[0,1]
    )
    return


@app.cell
def _(mo):
    mo.md(r"""## Alternative: varying batch size""")
    return


@app.cell
def _(REPO_ROOT, load_query_result_dir):
    df2 = load_query_result_dir(REPO_ROOT.joinpath("logs/runtime_outputs_04_06_09_40_27"))
    df2
    return (df2,)


@app.cell
def _(mo):
    dropdown2 = mo.ui.dropdown(options=list(range(1,11)), value=3, label="Batch duration")
    dropdown2
    return (dropdown2,)


@app.cell
def _(df2, dropdown2, pl):
    filtered_df2 = df2.filter(pl.col("expected_latency_day") == dropdown2.value).sort("rmsre")
    filtered_df2.select(["timestamp_min", "requested_epsilon", "batch_size", "rmsre", "oob_frac", "nc_frac", "c_frac", "qconvimp_frac", "qconv_frac", "qimp_frac", "rmsre_target", "tau_per_report"]).describe()
    return (filtered_df2,)


@app.cell
def _(df2, px):
    px.box(
        df2,
        y="rmsre",
        x="expected_latency_day",
        range_y=[0,1]
    )
    return


@app.cell
def _(mo):
    mo.md(r"""## Combining both""")
    return


@app.cell
def _(REPO_ROOT, load_query_result_dir):
    all = load_query_result_dir(REPO_ROOT.joinpath("logs/runtime_outputs_04_06_12_43_15"))
    return (all,)


@app.cell
def _(mo):
    # rmsre_dropdown = mo.ui.dropdown(options=[0.01, 0.025, 0.05, 0.075, 0.1, 0.15, 0.2, 0.25, 0.3], value=0.05, label="Target RMSRE")
    latency_dropdown = mo.ui.dropdown(options=list(range(1,11)), value=3, label="Batch duration")
    # rmsre_dropdown, 
    latency_dropdown
    return (latency_dropdown,)


@app.cell
def _(all, latency_dropdown, pl):
    fixed_latency = all.filter(
        pl.col("expected_latency_day") == latency_dropdown.value
    )
    fixed_latency.select(["timestamp_min", "requested_epsilon", "batch_size", "rmsre", "oob_frac", "nc_frac", "c_frac", "qconvimp_frac", "qconv_frac", "qimp_frac"]).describe()
    return (fixed_latency,)


@app.cell
def _(fixed_latency, latency_dropdown, pl, px):
    px.box(
        fixed_latency,
        y="rmsre",
        x="rmsre_target",
        range_y=[0,1],
        title=f"Varying RMSRE target for fixed latency {latency_dropdown.value} days and {len(fixed_latency.filter(pl.col('rmsre_target') == 0.05))} queries"
    )
    return


@app.cell
def _(fixed_latency, pl, px):
    px.line(fixed_latency.filter(pl.col("rmsre_target") == 0.05).sort("rmsre"),y="rmsre",range_y=[0,1])
    return


@app.cell
def _():


    return


if __name__ == "__main__":
    app.run()
