use std::{collections::hash_map, sync::LazyLock};

use pdslib::{
    budget::{pure_dp_filter::PureDPBudget, traits::Filter},
    util::hashmap::HashMap,
};

use crate::{
    attacks::attack_trait::{AnyAttack, Attack, AttackConfig},
    common_types::{Pds, Uri},
    datasets::dataset_trait::{Action, Dataset},
    querier::Querier,
};

/// A struct implementing a no-op attack, that just passes through actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoAttack;

impl Attack for NoAttack {
    fn file_suffix(&self) -> String {
        "NoAttack".to_string()
    }

    fn process_action(
        &self,
        action: Action,
        _dataset: &dyn Dataset,
        _device: &mut Pds<
            impl Filter<PureDPBudget, Error = anyhow::Error> + Clone,
        >,
        out: &mut Vec<Action>, // output buffer, pre-allocated
    ) {
        assert!(out.is_empty());
        out.push(action);
    }

    fn querier(&self, _uri: &Uri) -> &Querier {
        panic!("No querier available for NoAttack");
    }

    fn iter_queriers(&self) -> hash_map::Values<'_, Uri, Querier> {
        static EMPTY_QUERIER_MAP: LazyLock<HashMap<Uri, Querier>> =
            LazyLock::new(HashMap::default);
        EMPTY_QUERIER_MAP.values()
    }

    fn config(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn attack_id(&self) -> u64 {
        0
    }
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct NoAttackConfig;

impl AttackConfig for NoAttackConfig {
    fn create_attack(&self, _dataset: &dyn Dataset) -> AnyAttack {
        NoAttack.into()
    }
}
