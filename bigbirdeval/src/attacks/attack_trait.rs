use std::{collections::hash_map, fmt::Debug, hash::Hash};

use enum_dispatch::enum_dispatch;
use pdslib::{
    budget::{pure_dp_filter::PureDPBudget, traits::Filter},
    util::hashmap::{HashSet, RandomState},
};

use crate::{
    attacks::{
        attack_b::{AttackB, AttackBConfig},
        attack_c::{AttackC, AttackCConfig},
        no_attack::{NoAttack, NoAttackConfig},
    },
    common_types::{Pds, Uri},
    datasets::dataset_trait::{Action, Dataset},
    querier::Querier,
};

/// Trait representing an attack.
/// An attack can modify and inject impressions and conversions as
/// they are read from the dataset.
#[enum_dispatch]
pub trait Attack {
    /// The suffix appended to the end of the output file.
    fn file_suffix(&self) -> String;

    /// Take an action, and return a vector of modified/injected actions.
    /// If the attack does nothing, just return vec![action].
    fn process_action(
        &self,
        action: Action,
        dataset: &dyn Dataset,
        device: &mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        out: &mut Vec<Action>, // output buffer, pre-allocated
    );

    /// If the attack creates new queriers, it must store them, so that the
    /// runtime can retrieve them later.
    fn querier(&self, uri: &Uri) -> &Querier;
    fn iter_queriers(&self) -> hash_map::Values<'_, Uri, Querier>;

    fn sybils(&self) -> &HashSet<Uri> {
        // todo: only works with FxHash cause it's stateless
        static EMPTY_SYBILS: HashSet<Uri> = HashSet::with_hasher(RandomState);
        &EMPTY_SYBILS
    }

    /// Return the configuration in a format that will be written to the
    /// output file.
    fn config(&self) -> serde_json::Value;

    fn attack_id(&self) -> u64;

    fn cleanup_this_thread(&self) {}
}

#[enum_dispatch(Attack)]
pub enum AnyAttack {
    NoAttack(NoAttack),
    AttackB(AttackB),
    AttackC(AttackC),
}

/// A trait for this attack's configuration object.
/// Each attack has exactly one associated config type.
#[enum_dispatch]
pub trait AttackConfig: Into<AnyAttackConfig> {
    fn create_attack(&self, dataset: &dyn Dataset) -> AnyAttack;

    fn into_any(self) -> AnyAttackConfig {
        self.into()
    }
}

#[derive(Debug, Clone, PartialEq, Hash)]
#[enum_dispatch(AttackConfig)]
pub enum AnyAttackConfig {
    NoAttackConfig(NoAttackConfig),
    AttackBConfig(AttackBConfig),
    AttackCConfig(AttackCConfig),
}
