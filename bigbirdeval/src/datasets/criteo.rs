//! Loader for the CriteoPrivateAd fixtures (frozen parquet + heavy-querier
//! JSON). Reads two parquet files into an interleaved, chronologically-sorted
//! action stream, keeps only the "heavy" queriers, and precomputes the
//! site-popularity ranks the attacks need.

use std::{collections::hash_map, fs::File, path::Path, slice};

use kdam::tqdm;
use log::info;
use pdslib::{
    queries::ppa_histogram::RequestedBuckets,
    util::hashmap::{HashMap, HashSet},
};
use polars::{io::SerReader as _, prelude::ParquetReader};

use crate::{
    common_types::{Conversion, DeviceId, Uri, UriId},
    datasets::dataset_trait::{
        Dataset, ImpressionRow, LazyAction, device_action_counts,
        merge_imps_and_convs, rank_by_frequency,
    },
    querier::Querier,
    uriset_localizer::UriSetOrLocalizer,
    util::repo_root_dir,
};

pub struct CriteoDataset {
    actions: Vec<LazyAction>,
    queriers: HashMap<Uri, Querier>,
    source_ranks: HashMap<Uri, usize>,
    querier_ranks: HashMap<Uri, usize>,
    device_action_counts: HashMap<DeviceId, usize>,
    num_conversions: usize,
}

impl CriteoDataset {
    /// Load the full workload. `max_items` caps the merged action stream to
    /// its first N (chronological) actions — a smoke-test lever that exercises
    /// the pipeline end-to-end quickly and does NOT reproduce the paper's
    /// numbers. Pass `None` for the real workload.
    pub fn new(max_items: Option<usize>) -> anyhow::Result<Self> {
        // "20last" = Criteo days 11-30. These fixtures come from the seeded
        // ETL (seed=0) and are reproducible from the public Criteo dataset
        // via reproduce.sh; given them, runs here are deterministic.
        let data_dir = repo_root_dir().join("data/criteo");
        let px = "20last";

        let queriers = load_queriers(
            &data_dir.join(format!("{px}_heavy_convsites.json")),
        )?;

        // Parse each parquet in file order, then sort by timestamp. (The sort
        // is `unstable` but deterministic given the fixed file order; the
        // resulting action order feeds report batching, so it is load-bearing.)
        let mut impressions = parse_parquet(
            &data_dir.join(format!("{px}_impressions.pqt")),
            "Impressions",
            |get| {
                // The four most frequent `feature_0` values on day 1 map to
                // buckets 0-3; everything else collapses into bucket 4.
                let histogram_index =
                    match get("features_ctx_not_constrained_0") {
                        1158003214 => 0,
                        1668101868 => 1,
                        3880384520 => 2,
                        7687156 => 3,
                        _ => 4,
                    };
                ImpressionRow {
                    id: get("impression_id"),
                    timestamp: get("timestamp_min"),
                    epoch_number: get("day_int"),
                    histogram_index,
                    source_uri: Uri::benign(get("publisher_id") as UriId),
                    trigger_uri: Uri::benign(get("campaign_id") as UriId),
                    device_id: get("user_id"),
                }
            },
        )?;
        impressions.sort_unstable_by_key(|row| row.timestamp);

        let mut conversions = parse_parquet(
            &data_dir.join(format!("{px}_conversions.pqt")),
            "Conversions",
            |get| Conversion {
                epoch_id: get("day_int"),
                timestamp: get("timestamp_min"),
                querier: Uri::benign(get("campaign_id") as UriId),
                device_id: get("user_id"),
                user_action_id: Some(get("conversion_id")),
                source_uris: None, // use the querier's own source URIs
            },
        )?;
        conversions.sort_unstable_by_key(|conv| conv.timestamp);

        // Keep only impressions/conversions for the heavy (active) queriers.
        // An impression's single querier URI is its `trigger_uri`.
        let active: HashSet<Uri> = queriers.keys().copied().collect();
        impressions.retain(|row| active.contains(&row.trigger_uri));
        conversions.retain(|conv| active.contains(&conv.querier));

        // Every action id must be unique across impressions and conversions.
        // (Guaranteed by construction — `conversion_id = impression_id + c` —
        // but cheap to assert.)
        let mut seen: HashSet<u64> = HashSet::default();
        for row in &impressions {
            assert!(seen.insert(row.id), "duplicate action id {}", row.id);
        }
        for conv in &conversions {
            let id = conv.user_action_id.unwrap();
            assert!(seen.insert(id), "duplicate action id {id}");
        }

        let source_ranks =
            rank_by_frequency(&impressions, |row| row.source_uri);
        let querier_ranks =
            rank_by_frequency(&conversions, |conv| conv.querier);

        let mut actions = merge_imps_and_convs(impressions, conversions);

        // Smoke-test lever: keep only the first N chronological actions. Guard
        // against an N so small it strands every querier (no conversions ->
        // no reports -> empty plots), so the truncated run is still valid.
        if let Some(n) = max_items {
            actions.truncate(n);
            let convs = actions
                .iter()
                .filter(|a| matches!(a, LazyAction::Conversion(_)))
                .count();
            assert!(
                convs > 0,
                "--max-items {n} truncated away every conversion; \
                 use a larger N"
            );
            info!("Capped workload to {} actions ({convs} conversions)", n);
        }

        let num_conversions = actions
            .iter()
            .filter(|a| matches!(a, LazyAction::Conversion(_)))
            .count();
        let device_action_counts = device_action_counts(&actions);

        Ok(Self {
            actions,
            queriers,
            source_ranks,
            querier_ranks,
            num_conversions,
            device_action_counts,
        })
    }
}

/// Read a parquet in file order, calling `parse` on a per-row `u64` column
/// accessor. Every Criteo column is stored as an integer and read as `u64`.
fn parse_parquet<T>(
    path: &Path,
    name: &str,
    parse: impl Fn(&dyn Fn(&str) -> u64) -> T,
) -> anyhow::Result<Vec<T>> {
    let df = ParquetReader::new(File::open(path)?).finish()?;

    let col_index: HashMap<&str, usize> = df
        .get_column_names()
        .iter()
        .enumerate()
        .map(|(i, c)| (c.as_str(), i))
        .collect();

    let mut out = Vec::with_capacity(df.height());
    let mut row = df.get_row(0)?;
    for idx in tqdm!(0..df.height(), desc = name) {
        df.get_row_amortized(idx, &mut row)?;
        let get = |col: &str| -> u64 {
            row.0[col_index[col]].extract().unwrap_or_else(|| {
                panic!("column {col} is not a u64 in {name}")
            })
        };
        out.push(parse(&get));
    }
    info!("Loaded {name}: {} rows", out.len());
    Ok(out)
}

/// Parse the heavy-querier JSON: one entry per campaign that survived the
/// avg-conversions-per-day threshold, carrying its source sites and its
/// expected 5-bucket report shape.
fn load_queriers(path: &Path) -> anyhow::Result<HashMap<Uri, Querier>> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_reader(File::open(path)?)?;

    let mut queriers = HashMap::default();
    for entry in entries {
        let id = entry["campaign_id"].as_u64().unwrap() as UriId;
        let uri = Uri::benign(id);

        let source_uris: HashSet<Uri> = entry["source_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Uri::benign(v.as_u64().unwrap() as UriId))
            .collect();

        let querier_uris: HashSet<Uri> = [uri].into_iter().collect();

        let expected_report: Vec<f64> = entry["feature_0_distribution"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();

        queriers.insert(
            uri,
            Querier {
                uri,
                buckets_to_consider: RequestedBuckets::AllBuckets,
                avg_conversions_per_epoch: entry["avg_conversions_per_day"]
                    .as_u64()
                    .unwrap(),
                expected_report,
                source_uris: Some(UriSetOrLocalizer::new_multi_thread(
                    source_uris,
                )),
                querier_uris: UriSetOrLocalizer::new_multi_thread(querier_uris),
            },
        );
    }
    Ok(queriers)
}

impl Dataset for CriteoDataset {
    fn actions(&self) -> slice::Iter<'_, LazyAction> {
        self.actions.iter()
    }

    fn len(&self) -> usize {
        self.actions.len()
    }

    fn querier(&self, uri: &Uri) -> &Querier {
        self.queriers
            .get(uri)
            .unwrap_or_else(|| panic!("Querier not found for URI: {uri:?}"))
    }

    fn iter_queriers(&self) -> hash_map::Values<'_, Uri, Querier> {
        self.queriers.values()
    }

    fn source_rank(&self, uri: &Uri) -> Option<usize> {
        self.source_ranks.get(uri).copied()
    }

    fn querier_rank(&self, uri: &Uri) -> Option<usize> {
        self.querier_ranks.get(uri).copied()
    }

    fn num_conversions(&self) -> usize {
        self.num_conversions
    }

    fn device_action_counts(&self) -> &HashMap<DeviceId, usize> {
        &self.device_action_counts
    }
}
