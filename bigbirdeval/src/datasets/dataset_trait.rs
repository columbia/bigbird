//! The dataset abstraction and the action stream it produces.
//!
//! A dataset yields `LazyAction`s in chronological order. Impressions are held
//! as plain PODs (`ImpressionRow`) and materialized into full `Impression`s
//! (with pdslib's `Rc`-backed `UriSet`) only on demand, per worker thread —
//! that keeps the shared dataset `Send + Sync` without paying to store the
//! non-thread-safe `UriSet` up front.

use std::{collections::hash_map, slice};

use pdslib::{
    events::{ppa_event::PpaEvent, traits::EventUris, uri_set::UriSet},
    util::hashmap::HashMap,
};

use crate::{
    common_types::{
        BucketKey, Conversion, DeviceId, EpochId, ImpOrConv, Impression,
        Timestamp, Uri,
    },
    querier::Querier,
};

/// A dataset that can be streamed as chronological actions and queried for the
/// site-popularity ranks the attacks consume. `actions()` is called once per
/// experiment, from every worker thread, so it must be cheap and shareable.
#[allow(clippy::len_without_is_empty)]
pub trait Dataset: Sync {
    /// Impressions and conversions, interleaved in chronological order.
    fn actions(&self) -> slice::Iter<'_, LazyAction>;
    fn len(&self) -> usize;

    /// The benign querier behind a (benign) URI.
    fn querier(&self, uri: &Uri) -> &Querier;
    fn iter_queriers(&self) -> hash_map::Values<'_, Uri, Querier>;

    /// Site rank (1 = busiest). `source_rank` ranks impression source sites;
    /// `querier_rank` ranks conversion queriers. Attacks pick their targets by
    /// rank.
    fn source_rank(&self, uri: &Uri) -> Option<usize>;
    fn querier_rank(&self, uri: &Uri) -> Option<usize>;

    fn num_conversions(&self) -> usize;

    /// Per-device action count, used to balance devices across worker threads.
    fn device_action_counts(&self) -> &HashMap<DeviceId, usize>;

    /// Hook for a dataset with a thread-local cache to clean up on its own
    /// thread. No-op for Criteo.
    fn cleanup_this_thread(&self) {}
}

/// A materialized impression or conversion.
#[derive(Debug, Clone)]
pub enum Action {
    Impression(Impression),
    Conversion(Conversion),
}

impl ImpOrConv for Action {
    fn device_id(&self) -> DeviceId {
        match self {
            Action::Impression(i) => i.device_id,
            Action::Conversion(c) => c.device_id,
        }
    }

    fn epoch_id(&self) -> EpochId {
        match self {
            Action::Impression(i) => i.epoch_id(),
            Action::Conversion(c) => c.epoch_id(),
        }
    }

    fn timestamp(&self) -> Timestamp {
        match self {
            Action::Impression(i) => i.timestamp(),
            Action::Conversion(c) => c.timestamp(),
        }
    }

    fn user_action_id(&self) -> Option<u64> {
        match self {
            Action::Impression(i) => i.user_action_id(),
            Action::Conversion(c) => c.user_action_id(),
        }
    }
}

/// Everything needed to rebuild an `Impression`, in a `Send + Sync` POD form.
/// `trigger_uri` doubles as the querier URI (Criteo attributes each impression
/// to a single campaign that is both trigger and querier).
#[derive(Debug, Clone)]
pub struct ImpressionRow {
    pub id: u64,
    pub timestamp: Timestamp,
    pub epoch_number: EpochId,
    pub histogram_index: BucketKey,
    pub source_uri: Uri,
    pub trigger_uri: Uri,
    pub device_id: DeviceId,
}

impl ImpressionRow {
    fn build(&self) -> Impression {
        let querier_uris: UriSet<Uri> = [self.trigger_uri].into();
        let event = PpaEvent {
            id: self.id,
            timestamp: self.timestamp,
            epoch_number: self.epoch_number,
            histogram_index: self.histogram_index,
            uris: EventUris {
                source_uri: self.source_uri,
                trigger_uris: querier_uris.clone(),
                querier_uris,
            },
            user_action_id: Some(self.id),
            filter_data: 0,
        };
        Impression {
            device_id: self.device_id,
            event,
        }
    }
}

/// An action in the stored stream: a cheap impression row (materialized on
/// demand) or a conversion (cloned on demand).
#[derive(Debug, Clone)]
pub enum LazyAction {
    Impression(ImpressionRow),
    Conversion(Conversion),
}

impl LazyAction {
    pub fn get(&self) -> Action {
        match self {
            LazyAction::Impression(row) => Action::Impression(row.build()),
            LazyAction::Conversion(conv) => Action::Conversion(conv.clone()),
        }
    }

    pub fn device_id(&self) -> DeviceId {
        match self {
            LazyAction::Impression(row) => row.device_id,
            LazyAction::Conversion(conv) => conv.device_id,
        }
    }
}

/// Frequency ranks (1 = busiest) for the URI extracted from each item. Ties are
/// broken by hashmap iteration order — load-bearing, do not reorder.
pub fn rank_by_frequency<T>(
    items: &[T],
    get_uri: impl Fn(&T) -> Uri,
) -> HashMap<Uri, usize> {
    let mut counts: HashMap<Uri, i32> = HashMap::default();
    for item in items {
        *counts.entry(get_uri(item)).or_insert(0) += 1;
    }

    let mut busiest: Vec<Uri> = counts.keys().copied().collect();
    busiest.sort_unstable_by_key(|uri| -counts[uri]);

    busiest
        .into_iter()
        .enumerate()
        .map(|(rank, uri)| (uri, rank + 1))
        .collect()
}

/// Merge two already-sorted streams by timestamp. On a tie the IMPRESSION is
/// emitted before the CONVERSION (a conversion is only taken while it is
/// strictly earlier) — this ordering feeds the report batching and is
/// load-bearing.
pub fn merge_imps_and_convs(
    impressions: Vec<ImpressionRow>,
    conversions: Vec<Conversion>,
) -> Vec<LazyAction> {
    let mut actions = Vec::with_capacity(impressions.len() + conversions.len());
    let mut imps = impressions.into_iter().peekable();
    let mut convs = conversions.into_iter().peekable();

    while let (Some(imp), Some(conv)) = (imps.peek(), convs.peek()) {
        if conv.timestamp < imp.timestamp {
            actions.push(LazyAction::Conversion(convs.next().unwrap()));
        } else {
            actions.push(LazyAction::Impression(imps.next().unwrap()));
        }
    }
    actions.extend(imps.map(LazyAction::Impression));
    actions.extend(convs.map(LazyAction::Conversion));
    actions
}

/// Number of actions per device.
pub fn device_action_counts(
    actions: &[LazyAction],
) -> HashMap<DeviceId, usize> {
    let mut counts = HashMap::default();
    for action in actions {
        *counts.entry(action.device_id()).or_insert(0) += 1;
    }
    counts
}
