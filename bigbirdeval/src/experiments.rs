//! The paper's experiment suites. Each function returns the explicit list of
//! `Experiment`s for one `--experiment` value.
//!
//! Only the suites that produce a figure in the paper are here: `paper` (Fig.
//! 5) and the three panels of Fig. 6 (`big-bird-bc`,
//! `attacker-strength-num-sybils`, `attacker-strength-popularity`). The paper
//! evaluates two attacker strategies, Random (Attack B) and Omniscient (Attack
//! C).
//!
//! Only three capacity knobs are ever swept — `source_quota`, `global`, and
//! `quota_count` — which give the four system variants below. Everything else
//! is fixed in `default_runtime_config`.

use std::f64;

use pdslib::pds::quotas::StaticCapacities;

use crate::{
    attacks::{
        attack_b::AttackBConfig,
        attack_c::AttackCConfig,
        attack_trait::{AnyAttackConfig, AttackConfig},
        no_attack::NoAttackConfig,
    },
    config::{Experiment, RuntimeConfig},
};

const DEFAULT_SYBILS_PER_CONVERSION_PROPORTION: f64 = 0.35;

/// The knobs shared by every suite. `StaticCapacities::new` is positional:
/// (per_querier, global, trigger_quota, source_quota).
fn default_runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        capacities: StaticCapacities::new(1.0, 8.0, 1.0, 2.0),
        quota_count: Some(2),
        expected_latency_epochs: 10,
        min_batch_size: 5000,
        query_global_sensitivity: 1.0,
        rmsre_target: 0.05,
        tau_per_report: 0.05,
    }
}

/// The four system variants, derived from Big Bird by removing caps:
/// - **BB** (Big Bird): source_quota=2, global=8, quota_count=Some(2).
/// - **BBNQ**: BB with no source quota (source_quota=∞).
/// - **PPA** ("Attribution w/ global"): BBNQ with no domain cap.
/// - **CM** (CookieMonster, "Attribution w/o global"): PPA with no global.
struct Variants {
    bb: RuntimeConfig,
    bbnq: RuntimeConfig,
    ppa: RuntimeConfig,
    cm: RuntimeConfig,
}

fn system_variants() -> Variants {
    let bb = default_runtime_config();

    let mut bbnq = bb.clone();
    bbnq.capacities.source_quota = f64::INFINITY;

    let mut ppa = bbnq.clone();
    ppa.quota_count = None;

    let mut cm = ppa.clone();
    cm.capacities.global = f64::INFINITY;

    Variants { bb, bbnq, ppa, cm }
}

// --- Attack parameters ---

#[derive(Clone)]
struct CommonAttackParams {
    malicious_site_rank_start: u64,
    malicious_site_count: u64,
    num_redirections: u64,
    num_sybils: u64,
    genuine_first_chance: u32,
    malicious_sister_sites: bool,
}

impl Default for CommonAttackParams {
    fn default() -> Self {
        Self {
            malicious_site_rank_start: 1,
            malicious_site_count: 10,
            // Overwritten by `Experiment::new` from the domain cap; the value
            // here is never used.
            num_redirections: 0,
            num_sybils: 25,
            genuine_first_chance: 50,
            malicious_sister_sites: true,
        }
    }
}

impl CommonAttackParams {
    fn to_attack_b(&self) -> AttackBConfig {
        let mut attack = AttackBConfig {
            malicious_site_rank_start: self.malicious_site_rank_start,
            malicious_site_count: self.malicious_site_count,
            num_redirections: self.num_redirections,
            num_sybils: self.num_sybils,
            sybils_per_conversion: 0, // set below
            genuine_first_chance: self.genuine_first_chance,
            malicious_sister_sites: self.malicious_sister_sites,
        };
        attack.set_sybils_per_conversion_proportion(
            DEFAULT_SYBILS_PER_CONVERSION_PROPORTION,
        );
        attack
    }

    fn to_attack_c(&self) -> AttackCConfig {
        AttackCConfig {
            malicious_site_rank_start: self.malicious_site_rank_start,
            malicious_site_count: self.malicious_site_count,
            num_redirections: self.num_redirections,
            num_sybils: self.num_sybils,
            genuine_first_chance: self.genuine_first_chance,
            malicious_sister_sites: self.malicious_sister_sites,
        }
    }
}

// --- Suites ---

/// Fig. 5: 13 runs per attack (BBNQ, PPA, CM, and a 10-point BB `source_quota`
/// sweep), for NoAttack and AttackB. 26 runs.
pub fn paper() -> Vec<Experiment> {
    let v = system_variants();
    let attacks = [
        NoAttackConfig.into_any(),
        CommonAttackParams::default().to_attack_b().into_any(),
    ];

    let mut experiments = vec![];
    for attack in attacks {
        experiments.push(Experiment::new(v.bbnq.clone(), attack.clone()));
        experiments.push(Experiment::new(v.ppa.clone(), attack.clone()));
        experiments.push(Experiment::new(v.cm.clone(), attack.clone()));

        // BB sweeping the impression (source) quota. BB with qimp≈4 is roughly
        // a hypothetical batched PPA.
        for eps_qimp in [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 7.5, 8.0] {
            let mut rc = v.bb.clone();
            rc.capacities.source_quota = eps_qimp;
            experiments.push(Experiment::new(rc, attack.clone()));
        }
    }
    experiments
}

/// CM + PPA singletons, then a `quota_count` ∈ 1..=4 sweep over {BB, BBNQ}, all
/// under one attack. 10 runs.
fn quota_count_suite(attack: AnyAttackConfig) -> Vec<Experiment> {
    let v = system_variants();

    let mut experiments = vec![
        Experiment::new(v.cm.clone(), attack.clone()),
        Experiment::new(v.ppa.clone(), attack.clone()),
    ];
    for quota_count in 1..=4 {
        for base in [&v.bb, &v.bbnq] {
            let mut rc = base.clone();
            rc.quota_count = Some(quota_count);
            experiments.push(Experiment::new(rc, attack.clone()));
        }
    }
    experiments
}

/// Fig. 6a (domain-cap panel): AttackB and AttackC over the same `quota_count`
/// sweep, overlaid. 20 runs.
pub fn big_bird_bc() -> Vec<Experiment> {
    let b = CommonAttackParams::default().to_attack_b().into_any();
    let c = CommonAttackParams::default().to_attack_c().into_any();

    let mut experiments = quota_count_suite(b);
    experiments.extend(quota_count_suite(c));
    experiments
}

/// Fig. 6b/6c (attacker-strength panels): AttackB and AttackC on default BB,
/// sweeping one attacker-strength parameter. Builds one variation list, then
/// runs B over it and C over it.
fn attacker_strength(
    variations: impl Fn(CommonAttackParams) -> Vec<CommonAttackParams>,
) -> Vec<Experiment> {
    let bb = default_runtime_config();
    let variations = variations(CommonAttackParams::default());

    let mut experiments = vec![];
    for params in &variations {
        experiments
            .push(Experiment::new(bb.clone(), params.to_attack_b().into_any()));
    }
    for params in &variations {
        experiments
            .push(Experiment::new(bb.clone(), params.to_attack_c().into_any()));
    }
    experiments
}

/// Fig. 6b: sweep the number of sybil domains. 6 × {B, C} = 12 runs.
pub fn attacker_strength_num_sybils() -> Vec<Experiment> {
    attacker_strength(|base| {
        [5, 10, 25, 50, 75, 100]
            .into_iter()
            .map(|num_sybils| CommonAttackParams {
                num_sybils,
                ..base.clone()
            })
            .collect()
    })
}

/// Fig. 6c: sweep the popularity rank of the hijacked sites. 5 × {B, C} = 10
/// runs.
pub fn attacker_strength_popularity() -> Vec<Experiment> {
    attacker_strength(|base| {
        (1..=5)
            .map(|malicious_site_rank_start| CommonAttackParams {
                malicious_site_rank_start,
                ..base.clone()
            })
            .collect()
    })
}
