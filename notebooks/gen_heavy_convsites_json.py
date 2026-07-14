import sys
import json
from polars import read_parquet
import polars as pl
from bigbird.common import DATA_ROOT

prefix = sys.argv[1]  # e.g., "10days"
idf = read_parquet(DATA_ROOT.joinpath(f"criteo/{prefix}_impressions.pqt"))
cdf = read_parquet(DATA_ROOT.joinpath(f"criteo/{prefix}_conversions.pqt"))


idf = idf.drop_nulls(subset=["campaign_id", "user_id", "publisher_id"])


def map_to_five_buckets(x):
    mapping = {1158003214: 0, 1668101868: 1, 3880384520: 2, 7687156: 3}
    return mapping.get(x, 4)


f0 = idf.with_columns(
    feature_0=idf["features_ctx_not_constrained_0"].map_elements(
        map_to_five_buckets, return_dtype=pl.Int64
    )
)

feature_counts = (
    f0.group_by(["campaign_id", "feature_0"])
    .len("count")
    .sort("count", descending=True)
)

impsites = idf.group_by("campaign_id").agg(pl.col("publisher_id").unique())

ddf = (
    cdf.filter(
        (pl.col("is_landed") == 1)
        | (pl.col("is_sale") == 1)
        | (pl.col("is_click") == 1)
        | (pl.col("is_visit") == 1)
    )
    .group_by(("campaign_id", "day_int"))
    .len("n_conversions")
    .sort("n_conversions", descending=True)
)

heavy_campaigns = (
    ddf.group_by("campaign_id")
    .agg(pl.col("n_conversions").mean().alias("avg_conversions_per_day").cast(pl.Int64))
    .sort("avg_conversions_per_day", descending=True)
)
heavy_campaigns = heavy_campaigns.filter(pl.col("avg_conversions_per_day") >= 50)

conv_config = impsites.join(heavy_campaigns, on="campaign_id").sort(
    "avg_conversions_per_day", descending=True
)

dict_config = {
    row["campaign_id"]: {
        "source_ids": row["publisher_id"],
        "avg_conversions_per_day": row["avg_conversions_per_day"],
    }
    for row in conv_config.iter_rows(named=True)
}
for queried_id in dict_config.keys():
    counts = []
    for f in range(5):
        # terribly inefficient but well...
        d = feature_counts.filter(pl.col("campaign_id") == queried_id).filter(
            pl.col("feature_0") == f
        )["count"]
        c = d[0] if d.len() else 0
        # print(f"campaign {queried_id}, feature {f}: {c}")
        counts.append(c)
    dict_config[queried_id]["feature_0_counts"] = counts
    dict_config[queried_id]["feature_0_distribution"] = [
        c / sum(counts) for c in counts
    ]
    dict_config[queried_id]["campaign_id"] = queried_id

DATA_ROOT.joinpath(f"criteo/{prefix}_querier_config.json").write_text(
    json.dumps(dict_config)
)

queriers_list = list(dict_config.values())
DATA_ROOT.joinpath(f"criteo/{prefix}_heavy_convsites.json").write_text(
    json.dumps(queriers_list)
)
