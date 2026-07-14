//! Experiment configuration: the runtime knobs (`RuntimeConfig`), the
//! `Runtime` seam the online engine implements, and a single `Experiment`
//! (one runtime config + one attack config).
//!
//! Capacities live in pdslib's `StaticCapacities` (per_querier / global /
//! trigger_quota / source_quota, in that positional order); `f64::INFINITY`
//! disables a filter. We keep that type verbatim because it is what the PDS
//! filter storage is constructed from.

use std::{hash::Hash, path::PathBuf};

use pdslib::{
    budget::pure_dp_filter::PureDPBudget, pds::quotas::StaticCapacities,
};
use serde::Serialize;

use crate::{
    attacks::attack_trait::{AnyAttackConfig, Attack},
    common_types::FilterId,
    datasets::dataset_trait::Dataset,
};

/// Every knob that is fixed for the duration of one experiment run.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeConfig {
    pub capacities: StaticCapacities<FilterId, PureDPBudget>,
    pub quota_count: Option<usize>,
    pub expected_latency_epochs: u64,
    pub min_batch_size: u64,
    pub query_global_sensitivity: f64,
    pub rmsre_target: f64,
    pub tau_per_report: f64,
}

impl Hash for RuntimeConfig {
    /// Used only to derive a unique output filename per experiment. Hashing the
    /// JSON is coarse but collision-free across the swept configs, and keeping
    /// it here preserves the existing on-disk log filenames.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let json = serde_json::to_string(self)
            .expect("Failed to serialize RuntimeConfig for hashing");
        json.hash(state);
    }
}

/// The seam the online engine plugs into: a dataset, an attack, a config, and
/// where its logs go.
pub trait Runtime {
    type Dataset: Dataset;
    type Attack: Attack;

    fn dataset(&self) -> &Self::Dataset;
    fn attack(&self) -> &Self::Attack;
    fn config(&self) -> &RuntimeConfig;

    fn output_path(&self) -> PathBuf;
}

/// One point in a suite: a runtime config paired with an attack config.
#[derive(Debug, Clone, Hash)]
pub struct Experiment {
    pub runtime_config: RuntimeConfig,
    pub attack_config: AnyAttackConfig,
}

impl Experiment {
    pub fn new(
        runtime_config: RuntimeConfig,
        mut attack_config: AnyAttackConfig,
    ) -> Self {
        // The redirection count is always derived from the domain cap, never
        // set independently: with a domain cap of `c` an attacker gets `c + 2`
        // redirections; with no cap it gets a flat 20.
        let num_redirections = match runtime_config.quota_count {
            Some(c) => (c + 2) as u64,
            None => 20,
        };

        match &mut attack_config {
            AnyAttackConfig::AttackBConfig(c) => {
                c.num_redirections = num_redirections
            }
            AnyAttackConfig::AttackCConfig(c) => {
                c.num_redirections = num_redirections
            }
            AnyAttackConfig::NoAttackConfig(_) => {}
        }

        Self {
            runtime_config,
            attack_config,
        }
    }
}
