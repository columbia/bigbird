use std::collections::hash_map;

use pdslib::{
    budget::{pure_dp_filter::PureDPBudget, traits::Filter},
    events::{ppa_event::PpaEvent, traits::EventUris},
    queries::ppa_histogram::RequestedBuckets,
    util::hashmap::{HashMap, HashSet},
};
use rand::seq::IteratorRandom as _;
use serde::Serialize;

use crate::{
    attacks::{
        attack_trait::{AnyAttack, Attack, AttackConfig},
        scaffold::{Targeting, begin_attack},
    },
    common_types::{Conversion, Impression, Pds, Uri, UriId},
    datasets::dataset_trait::{Action, Dataset},
    querier::Querier,
    uriset_localizer::UriSetOrLocalizer,
};

#[derive(Serialize, Debug, Clone, PartialEq, Hash)]
pub struct AttackBConfig {
    /// How many of the busiest sites to "hijack" and inject malicious
    /// impressions/conversions before every organic impression from these
    /// busy sites.
    pub malicious_site_rank_start: u64,
    pub malicious_site_count: u64,

    /// number of malicious redirections after the original organic site visit.
    /// For each redirection, we inject a malicious impression and conversion.
    pub num_redirections: u64,

    /// number of sybils the attacker has access to.
    pub num_sybils: u64,

    /// for every conversion, how many sybils to list as impsites
    pub sybils_per_conversion: u64,

    /// chance that the genuine impression/conversion is first (vs last).
    pub genuine_first_chance: u32, // percent 0-100

    /// whether the malicious sites are sisters of benign sites, i.e. have same
    /// user visit patterns but different user-actions, or hijacked, i.e.
    /// benign sites redirect to malicious sites with same user-action.
    pub malicious_sister_sites: bool,
}

pub struct AttackB {
    pub cfg: AttackBConfig,
    pub queriers: HashMap<Uri, Querier>,
    pub sybils: UriSetOrLocalizer,
}

impl AttackConfig for AttackBConfig {
    fn create_attack(&self, _dataset: &dyn Dataset) -> AnyAttack {
        let mut sybils_set = HashSet::default();
        for sybil_id in 0..self.num_sybils {
            let uri = Uri::malicious(sybil_id as UriId);
            sybils_set.insert(uri);
        }
        let sybils = UriSetOrLocalizer::new_multi_thread(sybils_set.clone());

        let mut this = AttackB {
            cfg: self.clone(),
            queriers: HashMap::default(),
            sybils: sybils.clone(),
        };

        for sybil in sybils_set {
            this.new_malicious_querier(sybil);
        }

        this.into()
    }
}

impl AttackBConfig {
    fn targeting(&self) -> Targeting {
        Targeting {
            rank_start: self.malicious_site_rank_start,
            count: self.malicious_site_count,
            num_redirections: self.num_redirections,
            genuine_first_chance: self.genuine_first_chance,
            sister_sites: self.malicious_sister_sites,
        }
    }

    pub fn set_sybils_per_conversion_proportion(&mut self, proportion: f64) {
        self.sybils_per_conversion =
            (self.num_sybils as f64 * proportion).round().max(1.0) as u64;
    }
}

impl Attack for AttackB {
    fn file_suffix(&self) -> String {
        format!(
            "atkB_malicious={}+{}_sybils={},_redirects={}_perconv={:.2}",
            self.cfg.malicious_site_rank_start,
            self.cfg.malicious_site_count,
            self.cfg.num_sybils,
            self.cfg.num_redirections,
            self.cfg.sybils_per_conversion,
        )
    }

    fn process_action(
        &self,
        action: Action,
        dataset: &dyn Dataset,
        _device: &mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        out: &mut Vec<Action>, // output buffer, pre-allocated
    ) {
        let Some(mut ctx) =
            begin_attack(action, dataset, out, self.cfg.targeting())
        else {
            return; // not a target; passthrough already pushed to out
        };

        let device_id = ctx.device_id;
        let timestamp = ctx.timestamp;
        let epoch_number = ctx.epoch_number;
        let user_action_id = ctx.user_action_id;

        let all_sybils = self.sybils.get();
        let sybils_for_this_attack = all_sybils
            .iter()
            .copied()
            .choose_multiple(&mut ctx.rng, self.cfg.num_redirections as usize);

        let mut converted_sybils = HashSet::new();

        for &sybil in &sybils_for_this_attack {
            let event = PpaEvent {
                id: 0,
                uris: EventUris {
                    source_uri: sybil,
                    trigger_uris: all_sybils.clone(),
                    querier_uris: all_sybils.clone(),
                },
                timestamp,
                epoch_number,
                histogram_index: 0,
                user_action_id,
                filter_data: 0,
            };
            let impression = Impression { device_id, event };
            out.push(Action::Impression(impression));

            let mut imp_sybils_for_conversion = all_sybils
                .iter()
                .filter(|&s| {
                    !converted_sybils.contains(s)
                        && !sybils_for_this_attack.contains(s)
                })
                .copied()
                .choose_multiple(
                    &mut ctx.rng,
                    self.cfg.sybils_per_conversion as usize,
                );

            if imp_sybils_for_conversion.len()
                < self.cfg.sybils_per_conversion as usize
            {
                // we ran out of unused sybils. This could happen if we have
                // e.g. 3 redirections, so we shoudd use 1/3 of the sybils for
                // each of the 3 conversions, but
                // sybils_per_conversion is > 1/3 * num_sybils.
                // in this case, just re-use sybils from the already-used pile.
                let sybils_short = self.cfg.sybils_per_conversion as usize
                    - imp_sybils_for_conversion.len();

                imp_sybils_for_conversion.extend(
                    all_sybils
                        .iter()
                        .filter(|&s| {
                            !sybils_for_this_attack.contains(s)
                                && !imp_sybils_for_conversion.contains(s)
                        })
                        .copied()
                        .choose_multiple(&mut ctx.rng, sybils_short),
                );
            }

            converted_sybils.extend(imp_sybils_for_conversion.iter().copied());

            let conversion = Conversion {
                querier: sybil,
                epoch_id: epoch_number,
                timestamp,
                device_id,
                user_action_id,
                source_uris: Some(UriSetOrLocalizer::new_multi_thread(
                    HashSet::from_iter(imp_sybils_for_conversion),
                )),
            };
            out.push(Action::Conversion(conversion));
        }

        ctx.emit_genuine_last(out);
    }

    fn querier(&self, uri: &Uri) -> &Querier {
        self.queriers.get(uri).unwrap_or_else(|| {
            panic!("Malicious querier not found for URI: {uri:?}")
        })
    }

    fn iter_queriers(&self) -> hash_map::Values<'_, Uri, Querier> {
        self.queriers.values()
    }

    fn sybils(&self) -> &HashSet<Uri> {
        self.sybils.inner_set()
    }

    fn config(&self) -> serde_json::Value {
        let mut cfg = serde_json::to_value(&self.cfg).unwrap();

        // add sybils_per_conversion_proportion
        let sybils_per_conversion_proportion =
            self.cfg.sybils_per_conversion as f64 / self.cfg.num_sybils as f64;
        cfg["sybils_per_conversion_proportion"] =
            sybils_per_conversion_proportion.into();

        cfg
    }

    fn attack_id(&self) -> u64 {
        2
    }
}

impl AttackB {
    fn new_malicious_querier(&mut self, uri: Uri) {
        assert!(
            !self.queriers.contains_key(&uri),
            "Querier for URI {uri:?} already exists"
        );
        let querier = Querier {
            uri,
            source_uris: None, // each conversion specifies its own uris
            querier_uris: UriSetOrLocalizer::new_multi_thread(
                HashSet::from_iter([uri]),
            ),
            buckets_to_consider: RequestedBuckets::AllBuckets,

            // these aren't used for malicious queriers
            avg_conversions_per_epoch: 0,
            expected_report: vec![],
        };
        self.queriers.insert(uri, querier);
    }
}
