use pdslib::{
    budget::{pure_dp_filter::PureDPBudget, traits::Filter},
    util::hashmap::HashMap,
};

use crate::common_types::{DeviceId, EpochId, Pds};

pub struct DeviceStorage<'a, PDS> {
    pub devices: HashMap<DeviceId, PDS>,
    factory: Box<dyn Fn() -> PDS + 'a>,
}

impl<'a, PDS> DeviceStorage<'a, PDS> {
    pub fn new(factory: impl Fn() -> PDS + 'a) -> Self {
        Self {
            devices: HashMap::default(),
            factory: Box::new(factory),
        }
    }

    pub fn get_or_create(&mut self, device_id: DeviceId) -> &mut PDS {
        self.devices
            .entry(device_id)
            .or_insert_with(|| (self.factory)())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PDS> {
        self.devices.values_mut()
    }
}

impl<'a, F: Filter<PureDPBudget, Error = anyhow::Error> + Clone>
    DeviceStorage<'a, Pds<F>>
{
    pub fn garbage_collect(&mut self, epoch_to_remove: EpochId) {
        // garbage collect all events, actions, and filters from epoch gc_epoch
        // and earlier.
        // also clean up devices that have no events left.
        // Note: if we start logging device statistics we can't do that anymore!

        self.devices.retain(|_, device| {
            // remove all events in this epoch, and earlier.
            device.event_storage.epochs.remove(&epoch_to_remove);

            // and if this device has no events left, remove the device
            // entirely.
            !device.event_storage.epochs.is_empty()
        });

        for device in self.iter_mut() {
            // also remove all filters for this epoch. pdslib's flat storage is
            // keyed by FilterId (which carries the epoch), so GC is a retain.
            device
                .core
                .filter_storage
                .filters
                .retain(|fid, _| *fid.epoch_id() != epoch_to_remove);

            // and remove action quotas for this epoch
            device.core.action_storage.actions.retain(
                |_, user_action_state| {
                    user_action_state
                        .accessed_sites
                        .retain(|epoch, _| *epoch != epoch_to_remove);

                    // keep only if there are still accessed sites left
                    !user_action_state.accessed_sites.is_empty()
                },
            );
        }
    }
}
