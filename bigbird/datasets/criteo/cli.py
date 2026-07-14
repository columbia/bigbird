import typer
from pathlib import Path
from loguru import logger
import polars as pl


from bigbird.common import REPO_ROOT
from bigbird.datasets.criteo.preprocess import (
    load_days,
    resample_dataset,
    rename_and_cast_columns,
    generate_impressions_and_conversions,
)


app = typer.Typer()


@app.command()
def generate_benign(
    start_day: int = 1,
    end_day: int = 1,
    prefix: Path = REPO_ROOT.joinpath("data/criteo/"),
    name: str = "10days",
    seed: int = 0,
):
    original = load_days(list(range(start_day, end_day + 1)))
    resampled_df = resample_dataset(original, seed=seed)

    df = rename_and_cast_columns(resampled_df)

    idf, cdf = generate_impressions_and_conversions(df)

    imp_path = prefix.joinpath(f"{name}_impressions.pqt")
    conv_path = prefix.joinpath(f"{name}_conversions.pqt")

    prefix.mkdir(parents=True, exist_ok=True)
    idf.sort(pl.col("timestamp_min")).write_parquet(imp_path)
    cdf.sort(pl.col("timestamp_min")).write_parquet(conv_path)

    logger.info(f"Impressions saved to {imp_path}")
    logger.info(f"Conversions saved to {conv_path}")


if __name__ == "__main__":
    app()
