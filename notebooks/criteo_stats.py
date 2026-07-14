import marimo

__generated_with = "0.13.12"
app = marimo.App(width="medium")


@app.cell
def _():
    import polars as pl
    from bigbird.common import DATA_ROOT
    from bigbird.datasets.criteo.preprocess import load_days
    return DATA_ROOT, load_days, pl


@app.cell
def _(DATA_ROOT, pl):
    impressions = pl.read_parquet(
        DATA_ROOT.joinpath("criteo/10days_impressions.pqt")
    )
    impressions.describe()
    return (impressions,)


@app.cell
def _(DATA_ROOT, pl):
    conversions = pl.read_parquet(
        DATA_ROOT.joinpath("criteo/10days_conversions.pqt")
    )
    conversions.describe()
    return (conversions,)


@app.cell
def _(impressions):
    impressions.group_by("user_id").n_unique()["impression_id"].quantile(0.50)
    return


@app.cell
def _(impressions):
    impressions.group_by("user_id").n_unique()["impression_id"].quantile(0.9)
    return


@app.cell
def _(conversions):
    conversions.describe()
    return


@app.cell
def _(conversions):
    conversions.group_by("user_id").len("n_conversions")["n_conversions"].quantile(0.50)
    return


@app.cell
def _(conversions):
    conversions.group_by("user_id").len("n_conversions")["n_conversions"].quantile(0.90)
    return


@app.cell
def _(impressions):
    impressions["user_id"].n_unique()
    return


@app.cell
def _(conversions):
    conversions["user_id"].n_unique()
    return


@app.cell
def _(load_days):
    full_df = load_days(list(range(1, 31)))
    return (full_df,)


@app.cell
def _(full_df):
    full_df.describe()
    return


@app.cell
def _(full_df):
    full_df["campaign_id"].n_unique()
    return


@app.cell
def _(full_df):
    full_df["publisher_id"].n_unique()
    return


if __name__ == "__main__":
    app.run()
