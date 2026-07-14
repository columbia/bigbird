use pdslib::{
    budget::traits::{Filter, FilterCapacities, FilterStorage as _},
    util::hashmap::{HashMap, HashSet},
};

use crate::{
    common_types::{DeviceId, EpochId, FilterId, OnlinePds, Uri},
    logs::logger::DeviceStats,
    querier::REPORT_REQUEST_EPOCH_WINDOW,
    runtimes::device_storage::DeviceStorage,
};

/// Stats collected for a single device during the experiment run.
#[derive(Default)]
pub struct RunningStats {
    /// Total budget consumed from the Global filter across all epochs.
    global_consumed: f64,
    /// Total capacity of the Global filter across all epochs.
    global_capacity: f64,
    /// Set of Sybil URIs that were "used" (consumed some budget) on this
    /// device.
    sybils_used: HashSet<Uri>,
    /// Set of Sybil URIs that were "exhausted" (ran out of budget) on this
    /// device.
    sybils_exhausted: HashSet<Uri>,
}

/// Helper struct to collect and aggregate statistics from devices managed by a
/// worker thread.
///
/// This collector accumulates stats as devices are processed and garbage
/// collected. Since devices are removed from memory (garbage collected) when
/// they are no longer needed, we must capture their final statistics before
/// they are dropped.
pub struct StatsCollector {
    /// Active running stats for devices currently in memory.
    pub device_stats: HashMap<DeviceId, RunningStats>,
    /// Final stats for devices that have been garbage collected.
    pub finished_stats: Vec<DeviceStats>,
}

impl Default for StatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsCollector {
    pub fn new() -> Self {
        Self {
            device_stats: HashMap::default(),
            finished_stats: vec![],
        }
    }

    /// Collect stats for the given epoch and then garbage collect the device
    /// storage.
    ///
    /// This method should be called periodically when an epoch expires (e.g.,
    /// falls out of the report request window). It:
    /// 1. Updates `RunningStats` for all active devices with data from
    ///    `epoch_to_remove`.
    /// 2. Calls `garbage_collect` on `DeviceStorage` to remove old data.
    /// 3. If a device is completely removed (has no more events), moves its
    ///    `RunningStats` to `finished_stats`.
    pub fn collect_and_gc(
        &mut self,
        devices: &mut DeviceStorage<OnlinePds>,
        epoch_to_remove: EpochId,
        sybils: &[Uri],
    ) {
        // Collect stats
        for (&dev_id, device) in devices.devices.iter_mut() {
            let stats = self.device_stats.entry(dev_id).or_default();
            Self::collect_epoch_stats(device, epoch_to_remove, sybils, stats);
        }

        // GC
        devices.garbage_collect(epoch_to_remove);

        // Move stats for removed devices
        self.device_stats.retain(|dev_id, stats| {
            if !devices.devices.contains_key(dev_id) {
                self.finished_stats.push(DeviceStats {
                    global_usage_ratio: if stats.global_capacity > 0.0 {
                        stats.global_consumed / stats.global_capacity
                    } else {
                        0.0
                    },
                    sybils_exhausted: stats.sybils_exhausted.len() as u32,
                    sybils_used: stats.sybils_used.len() as u32,
                });
                false
            } else {
                true
            }
        });
    }

    /// Finalize stats collection.
    ///
    /// This method should be called at the end of the thread's execution. It
    /// collects stats for all remaining epochs (that haven't been GC'd yet)
    /// and returns the full list of `DeviceStats` for all devices handled
    /// by this collector.
    pub fn finish(
        mut self,
        devices: &mut DeviceStorage<OnlinePds>,
        last_epoch_id: EpochId,
        sybils: &[Uri],
    ) -> Vec<DeviceStats> {
        let start = if last_epoch_id > REPORT_REQUEST_EPOCH_WINDOW {
            last_epoch_id - REPORT_REQUEST_EPOCH_WINDOW + 1
        } else {
            1
        };
        for (&dev_id, device) in devices.devices.iter_mut() {
            let stats = self.device_stats.entry(dev_id).or_default();
            for e in start..=last_epoch_id {
                Self::collect_epoch_stats(device, e, sybils, stats);
            }
            self.finished_stats.push(DeviceStats {
                global_usage_ratio: if stats.global_capacity > 0.0 {
                    stats.global_consumed / stats.global_capacity
                } else {
                    0.0
                },
                sybils_exhausted: stats.sybils_exhausted.len() as u32,
                sybils_used: stats.sybils_used.len() as u32,
            });
        }
        self.finished_stats
    }

    /// Helper function to inspect the budget usage of a device for a specific
    /// epoch and update its `RunningStats`.
    ///
    /// It checks:
    /// - Global filter usage.
    /// - Usage of Sybil-specific filters (PerQuerier, TriggerQuota,
    ///   SourceQuota).
    #[allow(clippy::collapsible_if)]
    fn collect_epoch_stats(
        device: &mut OnlinePds,
        epoch_id: EpochId,
        sybils: &[Uri],
        stats: &mut RunningStats,
    ) {
        let filters = &mut device.core.filter_storage;
        // Global
        let global_fid = FilterId::Global(epoch_id);
        if let Ok(Some(filter)) = filters.get_filter(&global_fid)
            && let Ok(remaining) = filter.remaining_budget()
        {
            let rem: f64 = remaining;
            let cap: f64 = filters.capacities().capacity(&global_fid).unwrap();

            if cap.is_finite() {
                stats.global_consumed += cap - rem;
                stats.global_capacity += cap;
            } else {
                stats.global_capacity = f64::INFINITY;
            }
        }

        // Sybils
        for sybil in sybils {
            let mut used = false;
            let mut exhausted = false;

            let fids = [
                FilterId::PerQuerier(epoch_id, *sybil),
                FilterId::TriggerQuota(epoch_id, *sybil),
                FilterId::SourceQuota(epoch_id, *sybil),
            ];

            for fid in fids {
                if let Ok(Some(filter)) = filters.get_filter(&fid)
                    && let Ok(remaining) = filter.remaining_budget()
                {
                    used = true;
                    let rem: f64 = remaining;
                    let cap: f64 = filters.capacities().capacity(&fid).unwrap();
                    if cap.is_finite() {
                        if rem < 1e-9 {
                            exhausted = true;
                        }
                    }
                }
            }

            if used {
                stats.sybils_used.insert(*sybil);
            }
            if exhausted {
                stats.sybils_exhausted.insert(*sybil);
            }
        }
    }
}
