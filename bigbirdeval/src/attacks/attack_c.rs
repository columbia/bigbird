use std::collections::hash_map;

use pdslib::{
    budget::{
        pure_dp_filter::PureDPBudget,
        traits::{Filter, FilterStorage},
    },
    events::{ppa_event::PpaEvent, traits::EventUris},
    queries::ppa_histogram::RequestedBuckets,
    util::hashmap::{HashMap, HashSet},
};
use serde::Serialize;

use crate::{
    attacks::{
        attack_trait::{AnyAttack, Attack, AttackConfig},
        scaffold::{Targeting, begin_attack},
    },
    common_types::{
        Conversion, EpochId, FilterId, Impression, Pds, Uri, UriId,
    },
    datasets::dataset_trait::{Action, Dataset},
    querier::Querier,
    uriset_localizer::UriSetOrLocalizer,
};

#[derive(Serialize, Debug, Clone, PartialEq, Hash)]
pub struct AttackCConfig {
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

    /// chance that the genuine impression/conversion is first (vs last).
    pub genuine_first_chance: u32, // percent 0-100

    /// whether the malicious sites are sisters of benign sites, i.e. have same
    /// user visit patterns but different user-actions, or hijacked, i.e.
    /// benign sites redirect to malicious sites with same user-action.
    pub malicious_sister_sites: bool,
}

pub struct AttackC {
    pub cfg: AttackCConfig,
    pub queriers: HashMap<Uri, Querier>,
    pub sybils_vec: Vec<Uri>,
    pub sybils_set: UriSetOrLocalizer,
}

impl AttackConfig for AttackCConfig {
    fn create_attack(&self, _dataset: &dyn Dataset) -> AnyAttack {
        let mut sybils_vec = vec![];
        for sybil_id in 0..self.num_sybils {
            let uri = Uri::malicious(sybil_id as UriId);
            sybils_vec.push(uri);
        }
        let sybils_set: HashSet<Uri> = sybils_vec.iter().copied().collect();
        let sybils_set = UriSetOrLocalizer::new_multi_thread(sybils_set);

        let mut this = AttackC {
            cfg: self.clone(),
            queriers: HashMap::default(),
            sybils_vec,
            sybils_set,
        };

        for sybil in this.sybils_vec.clone() {
            this.new_malicious_querier(sybil);
        }

        this.into()
    }
}

impl Attack for AttackC {
    fn file_suffix(&self) -> String {
        format!(
            "atkC_malicious={}+{}_sybils={},_redirects={}",
            self.cfg.malicious_site_rank_start,
            self.cfg.malicious_site_count,
            self.cfg.num_sybils,
            self.cfg.num_redirections,
        )
    }

    fn process_action(
        &self,
        action: Action,
        dataset: &dyn Dataset,
        device: &mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        out: &mut Vec<Action>, // output buffer, pre-allocated
    ) {
        let Some(ctx) =
            begin_attack(action, dataset, out, self.cfg.targeting())
        else {
            return; // not a target; passthrough already pushed to out
        };

        let device_id = ctx.device_id;
        let timestamp = ctx.timestamp;
        let epoch_number = ctx.epoch_number;
        let user_action_id = ctx.user_action_id;

        let all_sybils = self.sybils_set.get();

        let mut sybils_with_registered_impressions = HashSet::new();
        if let Some(stored_events) =
            device.event_storage.epochs.get(&epoch_number)
        {
            for event in stored_events {
                if all_sybils.contains(&event.uris.source_uri) {
                    sybils_with_registered_impressions
                        .insert(event.uris.source_uri);
                }
            }
        }

        let mut sybils_with_impressions_and_budget_left = self
            .find_imp_sybils_with_budget_left(
                sybils_with_registered_impressions.iter().copied(),
                device,
                epoch_number,
                // assume conv-quota budget is required
                device.core.filter_storage.capacities.per_querier,
            );

        let sybils_with_no_impressions: Vec<_> = self
            .sybils_vec
            .iter()
            .filter(|s| !sybils_with_registered_impressions.contains(s))
            .collect();

        let imp_sybils_for_this_attack = sybils_with_no_impressions
            .into_iter()
            // if we run out of sybils with no impressions, use ones with
            // impressions too
            .chain(sybils_with_registered_impressions.iter())
            .take(self.cfg.num_redirections as usize);

        for &sybil in imp_sybils_for_this_attack {
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

            // now find a sybil who:
            // 1. is not used in this attack (pdslib prevents us from eating our
            //    own lunch)
            // 2. has an impression
            // 3. has source quota budget left

            let Some(imp_sybil) =
                sybils_with_impressions_and_budget_left.next()
            else {
                // ran out of sybils with impressions and budget left
                continue;
            };

            let conversion = Conversion {
                querier: sybil,
                epoch_id: epoch_number,
                timestamp,
                device_id,
                user_action_id,
                source_uris: Some(UriSetOrLocalizer::new_multi_thread(
                    HashSet::from_iter(vec![imp_sybil]),
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
        self.sybils_set.inner_set()
    }

    fn config(&self) -> serde_json::Value {
        serde_json::to_value(&self.cfg).unwrap()
    }

    fn attack_id(&self) -> u64 {
        3
    }
}

impl AttackCConfig {
    fn targeting(&self) -> Targeting {
        Targeting {
            rank_start: self.malicious_site_rank_start,
            count: self.malicious_site_count,
            num_redirections: self.num_redirections,
            genuine_first_chance: self.genuine_first_chance,
            sister_sites: self.malicious_sister_sites,
        }
    }
}

impl AttackC {
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

    /// For this device, how much budget does this filter ID have left?
    /// Returns None if has max budget left (i.e. filter not initialized)
    fn budget_left(
        &self,
        device: &mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        filter_id: &FilterId,
    ) -> Option<f64> {
        let filter_storage = &mut device.core.filter_storage;

        let Some(filter) = filter_storage.get_filter(filter_id).unwrap() else {
            // filter not initialized => it has max budget left
            return None;
        };

        let remaining_budget = filter.remaining_budget().unwrap();
        Some(remaining_budget)
    }

    fn find_sybils_with_budget_left<'d>(
        &'d self,
        filter_ids: impl Iterator<Item = FilterId> + 'd,
        device: &'d mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        budget_required: PureDPBudget,
    ) -> impl Iterator<Item = FilterId> + 'd {
        filter_ids.filter(move |filter_id| {
            match self.budget_left(device, filter_id) {
                Some(budget_left) => budget_left >= budget_required,
                None => true, // max budget left
            }
        })
    }

    fn find_imp_sybils_with_budget_left<'d>(
        &'d self,
        sybils: impl Iterator<Item = Uri> + 'd,
        device: &'d mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        epoch_id: EpochId,
        budget_required: PureDPBudget,
    ) -> impl Iterator<Item = Uri> + 'd {
        let filter_ids = sybils
            .map(move |sybil_uri| FilterId::SourceQuota(epoch_id, sybil_uri));

        self.find_sybils_with_budget_left(filter_ids, device, budget_required)
            .map(|filter_id| match filter_id {
                FilterId::SourceQuota(_, uri) => uri,
                _ => unreachable!(),
            })
    }
}
