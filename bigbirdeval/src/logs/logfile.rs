use std::{
    fs::File,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::Write as _,
    path::PathBuf,
};

use flate2::{Compression, write::GzEncoder};
use pdslib::{
    budget::pure_dp_filter::PureDPBudget, pds::quotas::StaticCapacities,
    util::hashmap::HashMap,
};
use serde::Serialize;

use crate::{
    attacks::attack_trait::Attack,
    common_types::{BucketKey, EpochId, FilterId, UriId},
    config::Runtime,
    datasets::dataset_trait::Dataset,
    logs::logger::Logger,
    util::{buckets_vec, minus_one_if_inf, run_logs_dir},
};

/// Rust struct representing the output log file format.
#[derive(Serialize, Debug, Clone)]
pub struct LogFile {
    pub runtime_config: RuntimeConfig,
    pub workload_config: WorkloadConfig,
    pub capacities: StaticCapacities<FilterId, PureDPBudget>,
    pub query_results: Vec<QueryResult>,
    pub queriers: Vec<QuerierLog>,
    pub attack_stats: Option<AttackStats>,
}

#[derive(Serialize, Debug, Clone)]
pub struct AttackStats {
    pub global_budget_depletion: Vec<f64>, // percentiles 0-100
    pub sybils_exhausted: Vec<f64>,        // percentiles 0-100
    pub sybils_exhausted_ratio: Vec<f64>,  // percentiles 0-100
    pub sybils_used: Vec<f64>,             // percentiles 0-100
    pub sybils_used_ratio: Vec<f64>,       // percentiles 0-100
    pub total_sybils: usize,
}

#[derive(Serialize, Debug, Clone)]
pub struct RuntimeConfig {
    pub eps_nc: f64,
    pub eps_c: f64,
    pub eps_qconv: f64,
    pub eps_qimp: f64,
    pub quota_count: i64, // -1 for disabled
    pub max_items: Option<u64>,
    pub log_path: Option<PathBuf>, // Optional path to save logs
    pub save_detailed_logs: bool,
    pub common_querier_config: CommonQuerierConfig,
    pub n_releases: Option<(u64, u64)>, // Online if None. Batched otherwise.
    pub public_info: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct CommonQuerierConfig {
    pub min_batch_size: usize,
    pub expected_latency_day: usize,
    pub query_global_sensitivity: f64,
    pub rmsre_target: f64,
    pub tau_per_report: f64,
}

#[derive(Default, Serialize, Debug, Clone)]
pub struct WorkloadConfig {
    pub first_day: u64,
    pub last_day: u64,
    pub impressions_path: PathBuf,
    pub conversions_path: PathBuf,
    pub querier_config_path: PathBuf,
    pub heavy_impsites_path: PathBuf,

    #[serde(flatten)]
    pub attack_config: serde_json::Value,
    pub attack_id: u64, /* 0 = NoAttack, 2 = AttackB (Random), 3 = AttackC
                         * (Omniscient) */
}

#[derive(Serialize, Debug, Clone)]
pub struct QueryResult {
    /// Number of the workload item that triggered aggregation
    pub item_number: u64,

    /// Time of the conversion that triggered aggregation
    pub timestamp_min: u64,

    pub querier_id: UriId,
    pub malicious_querier: bool,
    pub start_epoch: EpochId,
    pub end_epoch: EpochId,
    pub requested_epsilon: f64,
    pub noise_scale: f64,
    pub batch_size: usize,
    pub noisy_aggregation: Vec<f64>,
    pub filtered_aggregation: Vec<f64>,
    pub unfiltered_aggregation: Vec<f64>,
    pub dropped_epochs: u64,
    pub dropped_nc: u64,
    pub dropped_c: u64,
    pub dropped_qconv: u64,
    pub dropped_qimp: u64,
    pub dropped_qcount: u64,
}

#[derive(Serialize, Debug, Clone)]
pub struct QuerierLog {
    pub config: QuerierConfigLog,
    pub stats: QuerierStatsLog,
}

#[derive(Serialize, Debug, Clone)]
pub struct QuerierStatsLog {
    pub desired_batch_size: u64,
    pub aggregated_batch_size: u64,
    pub avg_conversions_per_epoch: u64,
    pub remainder_reports: usize,
}

#[derive(Serialize, Debug, Clone)]
pub struct QuerierConfigLog {
    pub conv_site_id: usize,
    pub batch_size: usize,
    pub buckets_to_consider: Vec<BucketKey>,
    pub query_global_sensitivity: f64,
    pub requested_epsilon: f64,
    pub malicious_querier: bool,
}

impl LogFile {
    pub fn build(runtime: &impl Runtime, logger: Logger) -> LogFile {
        let attack = runtime.attack();
        let dataset = runtime.dataset();
        let config = runtime.config();

        let attack_stats = logger.compute_attack_stats(attack.sybils().len());

        let mut observed_batch_sizes: HashMap<(UriId, bool), Vec<usize>> =
            HashMap::default();
        for res in &logger.query_results {
            observed_batch_sizes
                .entry((res.querier_id, res.malicious_querier))
                .or_default()
                .push(res.batch_size);
        }

        let common_querier_config = CommonQuerierConfig {
            min_batch_size: config.min_batch_size as usize,
            expected_latency_day: config.expected_latency_epochs as usize,
            query_global_sensitivity: config.query_global_sensitivity,
            rmsre_target: config.rmsre_target,
            tau_per_report: config.tau_per_report,
        };

        let runtime_config = RuntimeConfig {
            eps_nc: minus_one_if_inf(config.capacities.per_querier),
            eps_c: minus_one_if_inf(config.capacities.global),
            eps_qconv: minus_one_if_inf(config.capacities.trigger_quota),
            eps_qimp: minus_one_if_inf(config.capacities.source_quota),
            quota_count: config.quota_count.map_or(-1, |c| c as i64),
            max_items: None,
            log_path: Some(LogFile::path(runtime, &logger)),
            save_detailed_logs: false,
            common_querier_config,
            n_releases: None,
            public_info: true,
        };

        let workload_config = WorkloadConfig {
            attack_id: attack.attack_id(),
            attack_config: attack.config(),
            // other fields are unused
            first_day: 0,
            last_day: 0,
            impressions_path: PathBuf::new(),
            conversions_path: PathBuf::new(),
            querier_config_path: PathBuf::new(),
            heavy_impsites_path: PathBuf::new(),
        };

        let mut queriers = vec![];
        for querier in dataset.iter_queriers() {
            let aggregated_batch_size = querier.batch_size(config);
            let desired_batch_size = config.expected_latency_epochs
                * querier.avg_conversions_per_epoch;
            let remainder_reports = logger
                .querier_logs
                .get(&querier.uri)
                .map(|ql| ql.batch.num_reports)
                .unwrap_or(0);

            if let Some(batches) = observed_batch_sizes
                .get(&(querier.uri.id, querier.uri.malicious))
            {
                for &b in batches {
                    assert_eq!(
                        b as u64, aggregated_batch_size,
                        "Aggregated batch size mismatch for querier {:?}",
                        querier.uri
                    );
                }
            }

            let stats = QuerierStatsLog {
                desired_batch_size,
                aggregated_batch_size,
                avg_conversions_per_epoch: querier.avg_conversions_per_epoch,
                remainder_reports,
            };

            let config = QuerierConfigLog {
                conv_site_id: querier.uri.id as usize,
                batch_size: aggregated_batch_size as usize,
                buckets_to_consider: buckets_vec(&querier.buckets_to_consider),
                query_global_sensitivity: config.query_global_sensitivity,
                requested_epsilon: querier.requested_epsilon(config),
                malicious_querier: false,
            };
            queriers.push(QuerierLog { config, stats });
        }
        for querier in attack.iter_queriers() {
            let aggregated_batch_size = querier.batch_size(config);
            let desired_batch_size = config.expected_latency_epochs
                * querier.avg_conversions_per_epoch;
            let remainder_reports = logger
                .querier_logs
                .get(&querier.uri)
                .map(|ql| ql.batch.num_reports)
                .unwrap_or(0);

            if let Some(batches) = observed_batch_sizes
                .get(&(querier.uri.id, querier.uri.malicious))
            {
                for &b in batches {
                    assert_eq!(
                        b as u64, aggregated_batch_size,
                        "Aggregated batch size mismatch for querier {:?}",
                        querier.uri
                    );
                }
            }

            let stats = QuerierStatsLog {
                desired_batch_size,
                aggregated_batch_size,
                avg_conversions_per_epoch: querier.avg_conversions_per_epoch,
                remainder_reports,
            };

            let config = QuerierConfigLog {
                conv_site_id: querier.uri.id as usize * 10
                    + querier.uri.malicious as usize,
                batch_size: aggregated_batch_size as usize,
                buckets_to_consider: buckets_vec(&querier.buckets_to_consider),
                query_global_sensitivity: config.query_global_sensitivity,
                requested_epsilon: querier.requested_epsilon(config),
                malicious_querier: true,
            };
            queriers.push(QuerierLog { config, stats });
        }

        LogFile {
            runtime_config,
            workload_config,
            capacities: config.capacities.clone(),
            query_results: logger.query_results,
            queriers,
            attack_stats,
        }
    }

    pub fn path(runtime: &impl Runtime, logger: &Logger) -> PathBuf {
        let cfg = (runtime.config(), runtime.attack().config());
        let mut hasher = DefaultHasher::new();
        cfg.hash(&mut hasher);
        let cfg_hash = format!("{:x}", hasher.finish());

        run_logs_dir(
            logger.start,
            logger.exp_name.as_deref(),
            logger.output_dir.as_ref(),
        )
        .join(format!(
            "eps_c={c}_qimp={qimp}_aq={aq}_{attack}:{cfg_hash}.json.gz",
            c = minus_one_if_inf(runtime.config().capacities.global),
            qimp = minus_one_if_inf(runtime.config().capacities.source_quota),
            aq = runtime.config().quota_count.unwrap_or(0),
            attack = runtime.attack().file_suffix(),
        ))
    }

    /// Save the logs to a JSON file or output to stdout if no path is provided.
    pub fn save(&self, path: PathBuf) -> anyhow::Result<()> {
        // Create the directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let mut encoder = GzEncoder::new(file, Compression::default());
        let json_str = serde_json::to_string(&self)?;
        encoder.write_all(json_str.as_bytes())?;
        encoder.finish()?;

        Ok(())
    }
}
