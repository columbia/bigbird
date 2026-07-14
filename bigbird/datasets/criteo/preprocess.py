from huggingface_hub import snapshot_download
import polars as pl
from pathlib import Path
from collections import defaultdict
from loguru import logger
import hashlib

from bigbird.datasets.criteo.constants import (
    MINS_IN_DAY,
    COLUMNS_TO_KEEP,
    IMP_FEATURES,
    IMP_METADATA,
    CONV_SCHEMA,
)


def load_day(id: int) -> pl.DataFrame:
    # Cached
    dataset_path = snapshot_download(
        repo_id="criteo/CriteoPrivateAd",
        repo_type="dataset",
        allow_patterns=f"*/day_int={id}/*",
    )

    directory = Path(dataset_path).joinpath(f"data/day_int={id}")

    files = list(directory.glob("*.gz.parquet"))

    # Day 3 has a duplicate parquet file containing all rows in a single shard.
    # Skip it to avoid doubling every row.
    DUPLICATE_FILES = {"part-00000-d0cfd34a-794e-4f02-a177-a96d3f9483cf-c000.gz.parquet"}
    files = [f for f in files if f.name not in DUPLICATE_FILES]

    if not files:
        raise FileNotFoundError(f"No data files in {directory}")

    dfs = [prune_columns(pl.read_parquet(file)) for file in files]

    return pl.concat(dfs, how="vertical").with_columns(day_int=pl.lit(id))


def load_days(ids: list[int]) -> pl.DataFrame:
    dfs = [load_day(id) for id in ids]
    return pl.concat(dfs, how="vertical")


def resample_dataset(
    df: pl.DataFrame, count_displays_cutoff: int = 10, seed: int = 0
) -> pl.DataFrame:
    # `seed` makes the stratified subsample reproducible: the same public Criteo
    # input + the same seed yields the exact same fixtures on any machine.

    # Get the original (per-user) distribution from Criteo
    dataset_path = snapshot_download(
        repo_id="criteo/CriteoPrivateAd",
        repo_type="dataset",
        allow_patterns="*.csv",
    )
    correction = pl.read_csv(
        Path(dataset_path).joinpath("event_per_user_correction.csv"),
        separator=";",
    )

    # Compute our actual distribution on the subsampled dataset
    impressions_per_user = (
        df.group_by("user_id")
        .len("impressions_per_user")
        .group_by("impressions_per_user")
        .len("n_users")
        .sort("impressions_per_user")
    )

    # Compute the dataset size for a dataset following the original distribution,
    # where the number of users with x displays is the same as our actual dataset
    # Take the smallest dataset size to avoid having to upsample any bucket.
    # Usually this will be x = count_displays_cutoff, but not always.
    candidate_dataset_sizes = (
        impressions_per_user.filter(
            pl.col("impressions_per_user") <= count_displays_cutoff
        )
        .join(
            correction,
            left_on="impressions_per_user",
            right_on="cnt_displays",
        )
        .with_columns(
            candidate_dataset_size=pl.col("n_users")
            / (pl.col("nb_users_original") / 100)
        )  # original_proba_having_x_displays = original_n_users_having_x_displays / original_dataset_size,
        # where original_n_users_having_x_displays = n_users_having_x_displays
    )

    resampled_dataset_size = candidate_dataset_sizes["candidate_dataset_size"].min()

    # Compute the target sampling rate for each bucket
    target_sampling = candidate_dataset_sizes.with_columns(
        target_n_users=resampled_dataset_size * pl.col("nb_users_original") / 100
    ).with_columns(
        sampling_rate_within_bucket=pl.col("target_n_users") / pl.col("n_users"),
    )

    # Drop buckets below the cutoff in terms of population. Alternative would be upsampling them (with replacement)
    target_sampling = target_sampling.filter(
        pl.col("impressions_per_user") <= count_displays_cutoff
    )

    logger.info(f"Target sampling rates for each bucket: {target_sampling}")

    # Add sampling rate for each user, drop users that have too many impressions (i.e., buckets with small population).
    user_ids = (
        df.group_by("user_id")
        .len("impressions_per_user")
        .join(target_sampling, on="impressions_per_user", how="inner")
    )

    # Keep each user_id with a probability equal to the sampling rate of its
    # bucket. We loop over buckets explicitly rather than using
    # `group_by(...).map_groups(lambda b: b.sample(...))`: a Python UDF that
    # calls back into Polars runs on a Polars worker thread and deadlocks the
    # thread pool. Sorting by user_id first makes the seeded sample identical on
    # any machine regardless of upstream row order.
    buckets = dict(
        zip(
            target_sampling["impressions_per_user"],
            target_sampling["sampling_rate_within_bucket"],
        )
    )
    sampled_user_ids = pl.concat(
        user_ids.filter(pl.col("impressions_per_user") == n_impressions)
        .sort("user_id")
        .sample(fraction=rate, with_replacement=False, seed=seed)
        for n_impressions, rate in sorted(buckets.items())
    )
    sampled_df = df.filter(pl.col("user_id").is_in(sampled_user_ids["user_id"]))

    return sampled_df


def prune_columns(df: pl.DataFrame) -> pl.DataFrame:
    # Drop rows that have null values for important identifiers.

    logger.info(f"Size before dropping nulls and pruning columns: {df.shape}")
    df = df.select(COLUMNS_TO_KEEP)
    df = df.drop_nulls(subset=["campaign_id", "user_id", "publisher_id"])
    logger.info(f"Size after dropping nulls and pruning columns: {df.shape}")

    return df


def hex_string_to_int(s: str) -> int:
    # Output should fit in u64
    # Use MD5 to get a deterministic 64-bit integer hash from the string (hex or not)
    # The first 8 bytes of the MD5 digest are used to form an int64.
    # This avoids collisions seen with simple truncation of hex strings.
    hash_bytes = hashlib.md5(s.encode("utf-8")).digest()
    return int.from_bytes(hash_bytes[:8], byteorder="big", signed=False)


def rename_and_cast_columns(df: pl.DataFrame) -> pl.DataFrame:
    # TODO: make the publisher id positive, check collisions.

    df = df.rename({"id": "impression_id"}).with_columns(
        ((pl.col("day_int") - 1) * MINS_IN_DAY).alias("timestamp_min").cast(pl.Int64)
    )

    old_n_impressions_ids = df["impression_id"].n_unique()
    old_n_user_ids = df["user_id"].n_unique()
    old_n_publisher_ids = df["publisher_id"].n_unique()

    # Use integers instead of hexadecimal strings
    df = df.with_columns(
        impression_id=pl.col("impression_id").map_elements(
            hex_string_to_int, return_dtype=pl.UInt64
        ),
        user_id=pl.col("user_id").map_elements(
            hex_string_to_int, return_dtype=pl.UInt64
        ),
        publisher_id=pl.col("publisher_id").abs().cast(pl.UInt64),
    )

    # Check that our casting did not change the number of unique values
    if (
        old_n_impressions_ids != df["impression_id"].n_unique()
        or old_n_user_ids != df["user_id"].n_unique()
        or old_n_publisher_ids != df["publisher_id"].n_unique()
    ):
        logger.warning(
            f"Number of unique values changed after casting: {old_n_impressions_ids} -> {df['impression_id'].n_unique()}, {old_n_user_ids} -> {df['user_id'].n_unique()}, {old_n_publisher_ids} -> {df['publisher_id'].n_unique()}"
        )

    return df


def generate_impressions_and_conversions(
    df: pl.DataFrame,
) -> tuple[pl.DataFrame, pl.DataFrame]:
    conversions = defaultdict(list)
    MINS_IN_DAY = 24 * 60

    def append_conversion_metadata(impression, c):
        """Modifies the conversions dictionary in place."""
        impression_timestamp = int(impression["day_int"] - 1) * MINS_IN_DAY
        conversion_timestamp = (
            impression_timestamp + t if t >= 0 else impression_timestamp
        )
        conversions["timestamp_min"].append(conversion_timestamp)

        conversion_id = impression["impression_id"] + c
        conversions["conversion_id"].append(conversion_id)
        conversions["attributed_impression_id"].append(impression["impression_id"])
        conversions["day_int"].append(
            conversion_timestamp // MINS_IN_DAY + 1
        )  # Same as impression day in almost every case, except a couple percent of sale conversions.
        conversions["user_id"].append(impression["user_id"])
        conversions["campaign_id"].append(impression["campaign_id"])

    # We could vectorize if this is a bottleneck, but just takes a few seconds for 1M+ impressions.
    for impression in df.iter_rows(named=True):
        c = 1
        for t in impression["click_delay_after_display_array"]:
            append_conversion_metadata(impression, c)
            conversions["is_click"].append(1)
            conversions["is_landed"].append(0)
            conversions["is_visit"].append(0)
            conversions["is_sale"].append(0)
            conversions["nb_sales"].append(0)
            c += 1

        for t in impression["landed_click_delay_after_display_array"]:
            append_conversion_metadata(impression, c)
            conversions["is_click"].append(0)
            conversions["is_landed"].append(1)
            conversions["is_visit"].append(
                int(impression["is_visit"])
            )  # We treat visit as a property of a landed click
            conversions["is_sale"].append(0)
            conversions["nb_sales"].append(0)
            c += 1

        if impression["sale_delay_after_display_array"] is not None:
            # Other columns are not null
            for t in impression["sale_delay_after_display_array"]:
                append_conversion_metadata(impression, c)
                conversions["is_click"].append(0)
                conversions["is_landed"].append(0)
                conversions["is_visit"].append(0)
                conversions["is_sale"].append(1)
                conversions["nb_sales"].append(
                    int(impression["nb_sales"])
                )  # Same number of sales for all the sales from the same display.
                c += 1

    conversions_df = pl.DataFrame(
        conversions,
        schema=CONV_SCHEMA,
        strict=False,
    )

    impressions_df = df.select(IMP_METADATA + IMP_FEATURES)

    # Cast non-ID columns to Int64. IDs (impression_id, user_id, publisher_id) should be UInt64.
    cols_to_cast_int64 = [
        c
        for c in IMP_METADATA + IMP_FEATURES
        if c not in ["impression_id", "user_id", "publisher_id"]
    ]

    impressions_df = impressions_df.with_columns(
        [pl.col(col_name).cast(pl.Int64) for col_name in cols_to_cast_int64]
    )

    return impressions_df, conversions_df
