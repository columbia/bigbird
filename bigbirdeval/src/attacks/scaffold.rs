use rand::{Rng as _, rngs::StdRng};

use crate::{
    common_types::{DeviceId, EpochId, ImpOrConv as _, Timestamp},
    datasets::dataset_trait::{Action, Dataset},
};

/// The site-targeting and injection knobs shared by every attack config. Each
/// attack builds one of these from its own config and hands it to
/// [`begin_attack`].
pub struct Targeting {
    pub rank_start: u64,
    pub count: u64,
    pub num_redirections: u64,
    pub genuine_first_chance: u32,
    pub sister_sites: bool,
}

/// Context produced by [`begin_attack`] and consumed by the attack body.
/// Carries the per-action data every attack needs plus the seeded RNG and the
/// pending genuine action.
pub struct AttackScaffold {
    pub device_id: DeviceId,
    pub timestamp: Timestamp,
    pub epoch_number: EpochId,
    pub user_action_id: Option<u64>,
    pub rng: StdRng,
    genuine_first: bool,
    genuine_action: Option<Action>,
}

/// Runs the shared attack scaffold: the site-rank targeting gate, the
/// sister-site user-action offset, deterministic RNG seeding, and the
/// genuine-first coin flip. Returns `None` when the action is not a target (in
/// which case the passthrough has already been pushed to `out`); otherwise
/// returns the [`AttackScaffold`] the attack body needs, having already pushed
/// the genuine action to `out` if the coin flip landed genuine-first.
///
/// LOAD-BEARING FOR ORACLE REPRODUCIBILITY: the RNG is seeded from
/// `(device_id, timestamp, epoch_number, user_action_id)` — with the
/// sister-site offset already applied — and the `genuine_first` `gen_ratio`
/// draw is taken from it BEFORE the attack body performs any RNG use (e.g.
/// `choose_multiple`). Do not reorder the seed tuple or these draws: every
/// downstream number depends on the exact RNG call sequence.
pub fn begin_attack(
    action: Action,
    dataset: &dyn Dataset,
    out: &mut Vec<Action>,
    targeting: Targeting,
) -> Option<AttackScaffold> {
    assert!(out.is_empty());

    // determine if this site is within the top X sites
    let site_rank = match &action {
        Action::Impression(impression) => {
            let source_uri = &impression.event.uris.source_uri;
            dataset.source_rank(source_uri)
        }
        Action::Conversion(conversion) => {
            let querier_uri = &conversion.querier;
            dataset.querier_rank(querier_uri)
        }
    };

    let Some(site_rank) = site_rank else {
        out.push(action);
        return None; // no rank, do nothing
    };
    let site_rank = site_rank as u64;

    let malicious_site_rank_end = targeting.rank_start + targeting.count;
    let site_is_malicious = site_rank >= targeting.rank_start
        && site_rank < malicious_site_rank_end;

    if !site_is_malicious {
        out.push(action);
        return None; // only do attack if landed on malicious site
    }

    let total_actions_count = 1 + targeting.num_redirections * 2;
    out.reserve(total_actions_count as usize);

    let device_id = action.device_id();
    let timestamp = action.timestamp();
    let epoch_number = action.epoch_id();
    let mut user_action_id = action.user_action_id();

    if targeting.sister_sites {
        // make the malicious user-action distinct from the benign one
        if let Some(id) = user_action_id.as_mut() {
            *id += 1000000;
        }
    }

    // borrow checker workaround
    let mut action = Some(action);

    let mut rng = crate::util::deterministic_rng((
        device_id,
        timestamp,
        epoch_number,
        user_action_id,
    ));

    let genuine_first = match targeting.genuine_first_chance {
        0 => false,
        100 => true,
        chance => rng.gen_ratio(chance, 100),
    };
    if genuine_first {
        out.push(action.take().unwrap());
    }

    Some(AttackScaffold {
        device_id,
        timestamp,
        epoch_number,
        user_action_id,
        rng,
        genuine_first,
        genuine_action: action,
    })
}

impl AttackScaffold {
    /// Emits the genuine action last, unless the coin flip in [`begin_attack`]
    /// already emitted it first. Call after the attack body has pushed all of
    /// its injected actions.
    pub fn emit_genuine_last(mut self, out: &mut Vec<Action>) {
        if !self.genuine_first {
            out.push(self.genuine_action.take().unwrap());
        }
    }
}
