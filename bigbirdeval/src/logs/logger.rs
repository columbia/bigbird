use std::{mem::take, path::PathBuf};

use log::debug;
use pdslib::{
    pds::{core::DropEpochReason, private_data_service::PdsReport},
    queries::{histogram::HistogramReport, ppa_histogram::PpaHistogramRequest},
    util::hashmap::HashMap,
};
use rand::prelude::Distribution as _;
use statrs::distribution::Laplace;
use time::OffsetDateTime;

use crate::{
    common_types::{BucketKey, Conversion, EpochId, FilterId, Uri},
    config::RuntimeConfig,
    logs::logfile::{AttackStats, QueryResult},
    querier::Querier,
    util::buckets_vec,
};

/// Keeps track of the information that will be logged in the output file.
pub struct Logger {
    pub start: OffsetDateTime,
    pub exp_name: Option<String>,
    pub output_dir: Option<PathBuf>,
    pub num_actions: u64, // note: only counts conversions atm
    pub num_reports: u64,
    pub querier_logs: HashMap<Uri, QuerierLogs>,
    pub query_results: Vec<QueryResult>,
    pub device_stats: Vec<DeviceStats>,
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceStats {
    pub global_usage_ratio: f64,
    pub sybils_exhausted: u32,
    pub sybils_used: u32,
}

pub struct QuerierLogs {
    pub uri: Uri,
    pub batch: HistogramReportBatch,
    pub unfiltered_batch: HistogramReportBatch,
    pub batch_dropped_epochs: Vec<Vec<DropEpochReason<FilterId>>>,
}

#[derive(Debug, Default)]
pub struct HistogramReportBatch {
    pub global_sensitivity: f64,
    pub global_epsilon: f64,
    pub buckets_to_consider: Vec<BucketKey>,
    pub partial_aggregation: HashMap<BucketKey, f64>,
    pub num_reports: usize,
}

#[derive(Debug)]
pub struct AggregationRequest {
    pub batch: HistogramReportBatch,
    pub unfiltered_batch: HistogramReportBatch,

    /// For each report, the list of reasons why epochs were dropped
    pub batch_dropped_epochs: Vec<Vec<DropEpochReason<FilterId>>>,
}

#[derive(Debug)]
pub struct VectorAggregationResult {
    pub aggregation_output: Vec<f64>,
    pub aggregation_noisy_output: Vec<f64>,
}

#[derive(Clone, Copy)]
pub struct LoggerCtx<'a> {
    pub querier: &'a Querier,
    pub config: &'a RuntimeConfig,
    pub conversion: &'a Conversion,
    pub epoch_range: (EpochId, EpochId), // inclusive
}

impl Logger {
    pub fn new(
        start: OffsetDateTime,
        exp_name: Option<String>,
        output_dir: Option<PathBuf>,
    ) -> Self {
        Logger {
            start,
            exp_name,
            output_dir,
            num_actions: 0,
            num_reports: 0,
            querier_logs: HashMap::default(),
            query_results: vec![],
            device_stats: vec![],
        }
    }

    pub fn compute_attack_stats(
        &self,
        total_sybils: usize,
    ) -> Option<AttackStats> {
        let valid_stats: Vec<_> = self
            .device_stats
            .iter()
            .filter(|d| d.sybils_used > 0)
            .collect();

        if valid_stats.is_empty() || total_sybils == 0 {
            return None;
        }

        let mut global_budget_depletion: Vec<f64> =
            valid_stats.iter().map(|d| d.global_usage_ratio).collect();
        let mut sybils_exhausted: Vec<f64> = valid_stats
            .iter()
            .map(|d| d.sybils_exhausted as f64)
            .collect();
        let mut sybils_used: Vec<f64> =
            valid_stats.iter().map(|d| d.sybils_used as f64).collect();

        let mut sybils_exhausted_ratio: Vec<f64> = sybils_exhausted
            .iter()
            .map(|&x| x / total_sybils as f64)
            .collect();
        let mut sybils_used_ratio: Vec<f64> = sybils_used
            .iter()
            .map(|&x| x / total_sybils as f64)
            .collect();

        // sort and get percentiles
        let percentiles = |v: &mut Vec<f64>| -> Vec<f64> {
            v.sort_by(|a, b| a.total_cmp(b));
            (0..=100)
                .map(|p| {
                    let idx = (p as f64 / 100.0 * (v.len() - 1) as f64).round()
                        as usize;
                    v[idx]
                })
                .collect()
        };

        Some(AttackStats {
            global_budget_depletion: percentiles(&mut global_budget_depletion),
            sybils_exhausted: percentiles(&mut sybils_exhausted),
            sybils_exhausted_ratio: percentiles(&mut sybils_exhausted_ratio),
            sybils_used: percentiles(&mut sybils_used),
            sybils_used_ratio: percentiles(&mut sybils_used_ratio),
            total_sybils,
        })
    }

    pub fn process_report(
        &mut self,
        ctx: LoggerCtx<'_>,
        report: PdsReport<PpaHistogramRequest<Uri>>,
    ) -> anyhow::Result<()> {
        // if ctx.querier.uri.malicious {
        //     return Ok(()); // Don't log malicious reports
        // }

        self.num_actions += 1;
        self.num_reports += 1;

        let querier_logs = self.querier_logs(ctx);

        querier_logs.batch.add_report(report.filtered_report);
        querier_logs
            .unfiltered_batch
            .add_report(report.unfiltered_report);
        querier_logs
            .batch_dropped_epochs
            .push(report.drop_epoch_reasons);

        // is it time to aggregate?
        if querier_logs.batch.num_reports
            >= ctx.querier.batch_size(ctx.config) as usize
        {
            debug!("Time to aggregate: {:?}", querier_logs.batch);
            let aggregation_request = querier_logs.process_batch(ctx);
            self.aggregate_batch(ctx, aggregation_request);
        }

        Ok(())
    }

    fn querier_logs(&mut self, ctx: LoggerCtx<'_>) -> &mut QuerierLogs {
        // Get or create the querier logs for the current querier
        self.querier_logs
            .entry(ctx.querier.uri)
            .or_insert_with(|| QuerierLogs::new(ctx))
    }

    fn aggregate_batch(
        &mut self,
        ctx: LoggerCtx<'_>,
        aggregation_request: AggregationRequest,
    ) {
        let aggregation_result =
            self.aggregate_reports(&aggregation_request.batch);

        self.log_query_results(
            ctx.conversion.timestamp,
            ctx.conversion.querier,
            ctx.epoch_range.0,
            ctx.epoch_range.1,
            ctx.querier.requested_epsilon(ctx.config),
            aggregation_request,
            aggregation_result,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn log_query_results(
        &mut self,
        timestamp: u64,
        querier: Uri,
        start_epoch: EpochId,
        end_epoch: EpochId,
        requested_epsilon: f64,
        aggregation_request: AggregationRequest,
        aggregation_result: VectorAggregationResult,
    ) {
        let unfiltered_aggregation_result =
            aggregation_request.unfiltered_batch.aggregate_to_vec();
        assert!(!unfiltered_aggregation_result.is_empty());

        let (n_dropped, n_nc, n_c, n_qconv, n_qimp, n_qcount) =
            Self::process_dropped_epochs(
                aggregation_request.batch_dropped_epochs,
            );

        // TODO(later): also save noise and batch size from the request. Maybe
        // specialize to PpaHistogram.
        let batch_size = aggregation_request.batch.num_reports;
        let query_result = QueryResult {
            item_number: self.num_actions,
            timestamp_min: timestamp,
            querier_id: querier.id,
            malicious_querier: querier.malicious,
            start_epoch,
            end_epoch,
            requested_epsilon,
            noise_scale: aggregation_request.batch.get_noise_scale(),
            batch_size,
            noisy_aggregation: aggregation_result.aggregation_noisy_output,
            filtered_aggregation: aggregation_result.aggregation_output,
            unfiltered_aggregation: unfiltered_aggregation_result,
            dropped_epochs: n_dropped,
            dropped_nc: n_nc,
            dropped_c: n_c,
            dropped_qconv: n_qconv,
            dropped_qimp: n_qimp,
            dropped_qcount: n_qcount,
        };

        self.query_results.push(query_result);
    }

    #[allow(clippy::type_complexity)]
    fn process_dropped_epochs(
        batch_dropped_epochs: Vec<Vec<DropEpochReason<FilterId>>>,
    ) -> (
        u64, // n_dropped (total)
        u64, // n_nc
        u64, // n_c
        u64, // n_qconv
        u64, // n_qimp
        u64, // n_qcount
    ) {
        let mut n_dropped = 0; // total
        let mut n_nc = 0;
        let mut n_c = 0;
        let mut n_qconv = 0;
        let mut n_qimp = 0;
        let mut n_qcount = 0;

        for report_oob_filters in batch_dropped_epochs {
            // Aggregated counts with simple attribution

            if report_oob_filters.is_empty() {
                continue; // report had no dropped epochs
            }

            // At least one filter was OOB in the report, so we potentially
            // have a null report or biased one.
            n_dropped += 1;

            // We start by counting the OOB filters in each category, for
            // that particular report.
            let mut n_nc_report = 0;
            let mut n_c_report = 0;
            let mut n_qconv_report = 0;
            let mut n_qimp_report = 0;
            let mut n_qcount_report = 0;

            for drop_reason in &report_oob_filters {
                match drop_reason {
                    DropEpochReason::CountQuotaExceeded => n_qcount_report += 1,
                    DropEpochReason::OutOfBudget(filters) => {
                        for fid in filters {
                            match fid {
                                FilterId::PerQuerier(_, _) => n_nc_report += 1,
                                FilterId::Global(_) => n_c_report += 1,
                                FilterId::TriggerQuota(_, _) => {
                                    n_qconv_report += 1
                                }
                                FilterId::SourceQuota(_, _) => {
                                    n_qimp_report += 1
                                }
                            }
                        }
                    }
                }
            }

            // TODO(later): also log these raw values to check attribution.

            // Now we can attribute the whole report to a single OOB
            // category.
            if n_qcount_report > 0 {
                n_qcount += 1;
            } else if n_nc_report > 0 {
                n_nc += 1;
            } else if n_c_report > 0 {
                n_c += 1;
            } else if n_qimp_report > 0 {
                n_qimp += 1;
            } else if n_qconv_report > 0 {
                n_qconv += 1;
            }
        }

        assert_eq!(
            n_dropped,
            n_nc + n_c + n_qconv + n_qimp + n_qcount,
            "All dropped reports should be attributed to one category"
        );

        (n_dropped, n_nc, n_c, n_qconv, n_qimp, n_qcount)
    }

    pub fn aggregate_reports(
        &mut self,
        report_batch: &HistogramReportBatch,
    ) -> VectorAggregationResult {
        let aggregation_output = report_batch.aggregate_to_vec();

        let noise_scale = report_batch.get_noise_scale();
        assert!(
            noise_scale > 0.0,
            "Noise scale must be positive: {noise_scale}"
        );

        let laplace = Laplace::new(0.0, noise_scale).unwrap();
        let mut aggregation_noisy_output = vec![];

        for &value in &aggregation_output {
            let noise = laplace.sample(&mut rand::thread_rng());
            aggregation_noisy_output.push(value + noise);
        }

        VectorAggregationResult {
            aggregation_output,
            aggregation_noisy_output,
        }
    }
}

impl HistogramReportBatch {
    fn new(
        global_sensitivity: f64,
        global_epsilon: f64,
        buckets_to_consider: Vec<BucketKey>,
    ) -> Self {
        HistogramReportBatch {
            global_sensitivity,
            global_epsilon,
            buckets_to_consider,
            partial_aggregation: HashMap::default(),
            num_reports: 0,
        }
    }

    fn add_report(&mut self, report: HistogramReport<BucketKey>) {
        // NOTE: Aggregating on the fly for faster experiments. This would not
        // be possible with secret-shared reports encrypted towards
        // aggregators
        self.num_reports += 1; // Count all the reports, including null reports.
        for (bucket, attributed_value) in report.bin_values {
            *self.partial_aggregation.entry(bucket).or_default() +=
                attributed_value;
        }
    }

    fn get_noise_scale(&self) -> f64 {
        self.global_sensitivity / self.global_epsilon
    }

    /// Outputs a dense histogram aggregating the whole batch, as a vector
    fn aggregate_to_vec(&self) -> Vec<f64> {
        let mut aggregated_reports = vec![];

        // TODO: add histogram size or sorted buckets in the ReportBatch.
        let mut sorted_buckets: Vec<_> = self.buckets_to_consider.clone();
        sorted_buckets.sort_unstable();

        for bucket in sorted_buckets {
            let value = match self.partial_aggregation.get(&bucket) {
                Some(v) => *v,
                None => 0.0,
            };
            aggregated_reports.push(value);
        }

        aggregated_reports
    }
}

impl QuerierLogs {
    fn new(ctx: LoggerCtx<'_>) -> Self {
        QuerierLogs {
            uri: ctx.querier.uri,
            batch: HistogramReportBatch::new(
                ctx.config.query_global_sensitivity,
                ctx.querier.requested_epsilon(ctx.config),
                buckets_vec(&ctx.querier.buckets_to_consider),
            ),
            unfiltered_batch: HistogramReportBatch::new(
                ctx.config.query_global_sensitivity,
                ctx.querier.requested_epsilon(ctx.config),
                buckets_vec(&ctx.querier.buckets_to_consider),
            ),
            batch_dropped_epochs: vec![],
        }
    }

    fn process_batch(&mut self, ctx: LoggerCtx<'_>) -> AggregationRequest {
        // Extract the batch and replace it with an empty batch
        let batch_for_aggregator = take(&mut self.batch);
        let unfiltered_batch_for_aggregator = take(&mut self.unfiltered_batch);
        let batch_oob_filters = take(&mut self.batch_dropped_epochs);

        *self = Self::new(ctx);
        AggregationRequest {
            batch: batch_for_aggregator,
            unfiltered_batch: unfiltered_batch_for_aggregator,
            batch_dropped_epochs: batch_oob_filters,
        }
    }
}
