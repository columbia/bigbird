use std::hash::Hash;

use pdslib::{
    actions::hashmap_action_storage::HashMapActionStorage,
    budget::{
        hashmap_filter_storage::HashMapFilterStorage,
        pure_dp_filter::{PureDPBudget, PureDPBudgetFilter},
    },
    events::{
        hashmap_event_storage::HashMapEventStorage,
        ppa_event::PpaEvent,
        traits::{Event as _, EventUris},
    },
    pds::{aliases::PpaPds, quotas::StaticCapacities},
};
use rustc_hash::FxBuildHasher;
use serde::{Deserialize, Serialize};

use crate::uriset_localizer::UriSetOrLocalizer;

pub type Timestamp = u64;
pub type EpochId = u64;
pub type ActionId = u64;

pub type UriId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uri {
    pub id: UriId,
    pub malicious: bool,
}

impl Uri {
    pub fn benign(id: UriId) -> Self {
        Self {
            malicious: false,
            id,
        }
    }

    pub fn malicious(id: UriId) -> Self {
        Self {
            malicious: true,
            id,
        }
    }
}

impl Hash for Uri {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let as_u64 = (self.id as u64) | ((self.malicious as u64) << 32);
        state.write_u64(as_u64);
    }
}

impl nohash::IsEnabled for Uri {}

pub type FilterId = pdslib::pds::quotas::FilterId<EpochId, Uri>;

pub type DeviceId = u64;
pub type BucketKey = u64; // needs to be u64 for PpaHistogramRequest

pub type Event = PpaEvent<Uri>;

/// pdslib's own flat filter storage (`FilterId -> Filter`), keyed with FxHash.
/// The eval crate used to ship a shadow re-implementation; we deleted it and
/// use pdslib's directly, since that is the mechanism under study.
pub type FilterStorage<F> = HashMapFilterStorage<
    F,
    StaticCapacities<FilterId, PureDPBudget>,
    FxBuildHasher,
>;
pub type ActionStorage =
    HashMapActionStorage<ActionId, EpochId, Uri, FxBuildHasher>;
pub type EventStorage<Event> = HashMapEventStorage<Event, FxBuildHasher>;

pub type Pds<F> =
    PpaPds<FilterStorage<F>, ActionStorage, EventStorage<Event>, Uri>;
pub type OnlinePds = Pds<PureDPBudgetFilter>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impression {
    pub device_id: DeviceId,
    pub event: Event,
}

#[derive(Debug, Clone)]
pub struct Conversion {
    pub querier: Uri,
    pub epoch_id: EpochId,
    pub timestamp: Timestamp,
    pub device_id: DeviceId,
    pub user_action_id: Option<ActionId>,
    pub source_uris: Option<UriSetOrLocalizer>,
}

impl Impression {
    pub fn uris(&self) -> &EventUris<Uri> {
        &self.event.uris
    }
}

/// Common accessors over impressions and conversions.
pub trait ImpOrConv {
    fn device_id(&self) -> DeviceId;
    fn epoch_id(&self) -> EpochId;
    fn timestamp(&self) -> Timestamp;
    fn user_action_id(&self) -> Option<ActionId>;
}

impl ImpOrConv for Impression {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn epoch_id(&self) -> EpochId {
        self.event.epoch_id()
    }

    fn timestamp(&self) -> Timestamp {
        self.event.timestamp
    }

    fn user_action_id(&self) -> Option<ActionId> {
        self.event.user_action_id
    }
}

impl ImpOrConv for Conversion {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn epoch_id(&self) -> EpochId {
        self.epoch_id
    }

    fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    fn user_action_id(&self) -> Option<ActionId> {
        self.user_action_id
    }
}
