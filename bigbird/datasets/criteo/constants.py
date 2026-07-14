import polars as pl

MINS_IN_DAY = 24 * 60


COLUMNS_TO_KEEP = [
    "id",
    "user_id",
    "display_order",
    "sale_delay_after_display_array",
    "click_delay_after_display_array",
    "landed_click_delay_after_display_array",
    "campaign_id",
    "features_kv_not_constrained_1",
    "features_kv_not_constrained_2",
    "features_kv_not_constrained_3",
    "features_kv_not_constrained_4",
    "features_kv_not_constrained_5",
    "features_kv_not_constrained_6",
    "features_kv_not_constrained_7",
    "features_kv_not_constrained_8",
    "features_ctx_not_constrained_0",
    "features_ctx_not_constrained_1",
    "features_ctx_not_constrained_2",
    "features_ctx_not_constrained_3",
    "features_ctx_not_constrained_4",
    "features_ctx_not_constrained_5",
    "features_ctx_not_constrained_6",
    "features_ctx_not_constrained_7",
    "is_visit",
    "nb_sales",
    "is_clicked",
    "is_click_landed",
    "publisher_id",
]

IMP_METADATA = [
    "timestamp_min",
    "user_id",
    "impression_id",
    "day_int",
    "campaign_id",
    "publisher_id",
]

IMP_FEATURES = ["features_ctx_not_constrained_0"]

# IMP_FEATURES = [
#     "features_kv_not_constrained_1",
#     "features_kv_not_constrained_2",
#     "features_kv_not_constrained_3",
#     "features_kv_not_constrained_4",
#     "features_kv_not_constrained_5",
#     "features_kv_not_constrained_6",
#     "features_kv_not_constrained_7",
#     "features_kv_not_constrained_8",
#     "features_ctx_not_constrained_0",
#     "features_ctx_not_constrained_1",
#     "features_ctx_not_constrained_2",
#     "features_ctx_not_constrained_3",
#     "features_ctx_not_constrained_4",
#     "features_ctx_not_constrained_5",
#     "features_ctx_not_constrained_6",
#     "features_ctx_not_constrained_7",
# ]

CONV_SCHEMA = {
    "timestamp_min": pl.Int64,
    "user_id": pl.UInt64,
    "conversion_id": pl.UInt64,
    "day_int": pl.Int64,
    "campaign_id": pl.Int64,
    "is_click": pl.Int64,
    "is_landed": pl.Int64,
    "is_visit": pl.Int64,
    "is_sale": pl.Int64,
    "nb_sales": pl.Int64,
    "attributed_impression_id": pl.UInt64,
}
