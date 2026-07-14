//! A querier is a benign ad-tech site (or, under attack, a malicious one) that
//! turns a conversion into a differentially-private report request: which
//! epochs to attribute over, and how much epsilon to spend.

use pdslib::queries::{
    ppa_histogram::{
        PpaHistogramConfig, PpaHistogramRequest, PpaRelevantEventSelector,
        RequestedBuckets,
    },
    traits::ReportRequestUris,
};

use crate::{
    common_types::{BucketKey, Conversion, Uri},
    config::RuntimeConfig,
    uriset_localizer::UriSetOrLocalizer,
};

/// How many epochs *before* the conversion epoch a report may attribute over.
pub const REPORT_REQUEST_EPOCH_WINDOW: u64 = 2;
/// Number of histogram buckets (the 5-way `feature_0` map).
pub const HISTOGRAM_SIZE: u64 = 5;

#[derive(Debug)]
pub struct Querier {
    pub uri: Uri,
    pub buckets_to_consider: RequestedBuckets<BucketKey>,
    pub avg_conversions_per_epoch: u64,
    /// Expected per-report probability mass over the 5 buckets; sets the noise
    /// scale a benign querier needs to hit its RMSRE target.
    pub expected_report: Vec<f64>,
    /// Sources this querier attributes to. `None` on a conversion means "use
    /// the querier's own source set".
    pub source_uris: Option<UriSetOrLocalizer>,
    pub querier_uris: UriSetOrLocalizer, // just UriSet<[self.uri]>, cached
}

impl Querier {
    /// Smallest batch that meets the latency target; rounded up to
    /// `min_batch_size` even if that means higher latency.
    pub fn batch_size(&self, config: &RuntimeConfig) -> u64 {
        let desired =
            config.expected_latency_epochs * self.avg_conversions_per_epoch;
        desired.max(config.min_batch_size)
    }

    /// Epsilon this querier asks for per report request.
    pub fn requested_epsilon(&self, config: &RuntimeConfig) -> f64 {
        if self.uri.malicious {
            // A malicious querier grabs the maximum allowed.
            return config.capacities.per_querier;
        }

        let noise_scale = compute_noise_scale_from_rmsre_tau(
            config.rmsre_target,
            &self.expected_report,
            self.batch_size(config),
            config.tau_per_report,
        );
        config.query_global_sensitivity / noise_scale
    }

    pub fn report_request(
        &self,
        conversion: &Conversion,
        config: &RuntimeConfig,
    ) -> PpaHistogramRequest<Uri> {
        assert_eq!(conversion.querier, self.uri);

        let epoch_id = conversion.epoch_id;
        // ANOMALY (preserved bit-for-bit): the request spans 4 epochs
        // [E-3, E] (E - (WINDOW+1) ..= E), but garbage collection only retains
        // [E-1, E]. The two oldest requested epochs therefore always see no
        // events. This is intentional for reproduction; do not "fix" it.
        let histogram_config = PpaHistogramConfig {
            start_epoch: epoch_id
                .saturating_sub(REPORT_REQUEST_EPOCH_WINDOW + 1),
            end_epoch: epoch_id,
            attributable_value: 1.0,
            max_attributable_value: config.query_global_sensitivity,
            requested_epsilon: self.requested_epsilon(config),
            histogram_size: HISTOGRAM_SIZE,
        };

        let source_uris = if let Some(localizer) = &conversion.source_uris {
            localizer.get()
        } else if let Some(source_uris) = &self.source_uris {
            source_uris.get()
        } else {
            panic!(
                "No source URIs available for conversion {:?} and querier {:?}",
                conversion, self.uri
            );
        };

        let request_uris = ReportRequestUris {
            trigger_uri: self.uri,
            source_uris,
            querier_uris: self.querier_uris.get(),
        };
        let mut selector = PpaRelevantEventSelector::new(request_uris);
        selector.user_action_id = conversion.user_action_id;

        PpaHistogramRequest::new(&histogram_config, selector).unwrap()
    }
}

/// Laplace noise scale a benign querier needs for its target RMSRE, given the
/// expected report shape and batch size. Kept as an explicit left-to-right sum
/// — the arithmetic order is load-bearing for bit-exact reproduction.
fn compute_noise_scale_from_rmsre_tau(
    rmsre: f64,
    expected_report: &[f64],
    batch_size: u64,
    tau_per_report: f64,
) -> f64 {
    // s = Σ_i 1 / max(tau_per_report, p_i)^2
    let s: f64 = expected_report
        .iter()
        .map(|&p| 1.0 / p.max(tau_per_report).powi(2))
        .sum();

    // x = sqrt((2/m) * s) / batch_size
    let m = expected_report.len() as f64;
    let x = ((2.0 / m) * s).sqrt() / batch_size as f64;

    // b = rmsre / x
    rmsre / x
}
