use std::{path::PathBuf, thread};

use pdslib::{
    budget::traits::FilterStorage as _,
    pds::private_data_service::PdsReport,
    queries::{
        ppa_histogram::PpaHistogramRequest, traits::EpochReportRequest as _,
    },
    util::hashmap::HashMap,
};
use rustc_hash::FxBuildHasher;
use time::OffsetDateTime;

use crate::{
    attacks::attack_trait::{AnyAttack, AnyAttackConfig, Attack, AttackConfig},
    common_types::{
        ActionStorage, Conversion, DeviceId, EpochId, EventStorage,
        FilterStorage, ImpOrConv, OnlinePds, Pds, Uri,
    },
    config::{Runtime, RuntimeConfig},
    datasets::dataset_trait::{Action, Dataset, LazyAction},
    logs::{
        logfile::LogFile,
        logger::{DeviceStats, Logger, LoggerCtx},
    },
    progress::ProgressManager,
    querier::{Querier, REPORT_REQUEST_EPOCH_WINDOW},
    runtimes::{
        device_storage::DeviceStorage, stats_collector::StatsCollector,
    },
};

/// The online runtime, that will process conversions as they come in.
pub struct OnlineRuntime<'d, D: Dataset> {
    pub dataset: &'d D,
    pub attack: AnyAttack,
    pub config: RuntimeConfig,
    pub device_factory: Box<dyn Fn() -> OnlinePds + Send + Sync>,
    pub logger_factory: Box<dyn Fn() -> Logger + Send + Sync>,
}

/// One report emitted by a worker, tagged with its position in the single-
/// threaded dataset order (`action_num` = global action index, `sub_index` =
/// per-action injection order). The collector sorts by that pair.
struct ReportItem<'d> {
    action_num: usize,
    sub_index: u32,
    conversion: Conversion,
    querier: &'d Querier,
    epoch_range: (EpochId, EpochId),
    report: PdsReport<PpaHistogramRequest<Uri>>,
}

impl<'d, D: Dataset> OnlineRuntime<'d, D> {
    pub fn new(
        dataset: &'d D,
        attack_config: AnyAttackConfig,
        config: RuntimeConfig,
        start: OffsetDateTime,
        exp_name: Option<String>,
        output_dir: Option<PathBuf>,
    ) -> Self {
        let attack = attack_config.create_attack(dataset);

        let capacities = config.capacities.clone();
        let device_factory = move || {
            const CAP: usize = 0;
            Pds::new(
                FilterStorage::new(capacities.clone()).unwrap(),
                ActionStorage::with_hashmap_capacity(config.quota_count, CAP),
                EventStorage::with_hashmap_capacity(CAP),
            )
        };

        Self {
            dataset,
            attack,
            config,
            device_factory: Box::new(device_factory),
            logger_factory: Box::new(move || {
                Logger::new(start, exp_name.clone(), output_dir.clone())
            }),
        }
    }

    /// Pin each device to one worker thread using Longest Processing Time
    /// (LPT) greedy balancing: sort devices by action count (descending),
    /// assign each to the currently least-loaded thread. Deterministic.
    fn compute_device_assignments(
        &self,
        num_threads: usize,
    ) -> HashMap<DeviceId, usize, FxBuildHasher> {
        let mut device_map = HashMap::with_hasher(FxBuildHasher);
        let mut thread_loads = vec![0; num_threads];

        let counts = self.dataset.device_action_counts();
        let mut devices: Vec<_> = counts.iter().collect();
        devices.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(**count));

        for (&device_id, &count) in devices {
            let (thread_id, _) = thread_loads
                .iter()
                .enumerate()
                .min_by_key(|(_, load)| **load)
                .expect("num_threads must be > 0");

            device_map.insert(device_id, thread_id);
            thread_loads[thread_id] += count;
        }

        device_map
    }

    /// Process one worker's slice of the action stream (all actions for a
    /// disjoint set of devices) and return its emitted reports plus the
    /// per-device budget stats. No shared mutable state, no channels.
    fn process_partition(
        &'d self,
        actions: &[(usize, &'d LazyAction)],
        sybils: &[Uri],
        progress: &ProgressManager,
        bar_idx: usize,
    ) -> (Vec<ReportItem<'d>>, Vec<DeviceStats>) {
        let mut devices = DeviceStorage::new(&*self.device_factory);
        let mut stats_collector = StatsCollector::new();
        let mut reports = Vec::new();

        progress.init_experiment(bar_idx, actions.len(), "Worker".into());
        let update_freq = 4096;
        let mut processed_count = 0;

        // optimization: re-use the same vector to avoid allocations
        let mut processed_actions = vec![];

        // optimization:
        // since all reports are only for the past 2 epochs, garbage collect
        // old impressions/events as we go along
        let mut last_epoch_id = 0;

        for &(action_num, lazy_action) in actions {
            let action = lazy_action.get();
            let device_id = action.device_id();

            // garbage collect old epochs if needed
            let epoch_id = action.epoch_id();
            if epoch_id > last_epoch_id {
                if epoch_id > REPORT_REQUEST_EPOCH_WINDOW {
                    let epoch_to_remove =
                        epoch_id - REPORT_REQUEST_EPOCH_WINDOW;

                    stats_collector.collect_and_gc(
                        &mut devices,
                        epoch_to_remove,
                        sybils,
                    );
                }
                last_epoch_id = epoch_id;
            }

            processed_count += 1;
            if processed_count >= update_freq {
                progress.update_add(bar_idx, processed_count, true);
                processed_count = 0;
            }

            let device = devices.get_or_create(device_id);

            self.attack.process_action(
                action,
                self.dataset,
                device,
                &mut processed_actions,
            );

            // per-action injection order, so (action_num, sub_index) is a
            // globally unique, deterministic key for every emitted report.
            let mut sub_index: u32 = 0;
            for action in processed_actions.drain(..) {
                // optimization: assume all injected actions are for the same
                // device as the original action.
                // this allows us to avoid looking up the device multiple times.
                assert_eq!(device_id, action.device_id());

                // optimization: assume injected events never go back in time.
                // this allows the garbage collection above to work correctly.
                assert!(action.epoch_id() >= epoch_id);

                match action {
                    Action::Impression(impression) => {
                        device.register_event(impression.event).unwrap();
                    }

                    Action::Conversion(conversion) => {
                        let querier_uri = conversion.querier;
                        let querier = match querier_uri.malicious {
                            false => self.dataset.querier(&querier_uri),
                            true => self.attack.querier(&querier_uri),
                        };

                        let request =
                            querier.report_request(&conversion, &self.config);
                        let report = device
                            .compute_report(&request, conversion.user_action_id)
                            .unwrap();

                        let epochs = request.epoch_ids();
                        let start_epoch = *epochs.iter().min().unwrap();
                        let end_epoch = *epochs.iter().max().unwrap();

                        reports.push(ReportItem {
                            action_num,
                            sub_index,
                            conversion,
                            querier,
                            epoch_range: (start_epoch, end_epoch),
                            report,
                        });
                        sub_index += 1;
                    }
                }
            }

            assert!(processed_actions.is_empty());
        }

        if processed_count > 0 {
            progress.update_add(bar_idx, processed_count, true);
        }

        let stats = stats_collector.finish(&mut devices, last_epoch_id, sybils);

        progress.finish_experiment(bar_idx);

        (reports, stats)
    }

    pub fn run(
        self,
        experiment_num: usize,
        _total_experiments: usize,
        num_threads: usize,
        progress_manager: ProgressManager,
    ) -> anyhow::Result<thread::JoinHandle<anyhow::Result<()>>> {
        let mut logger = (self.logger_factory)();

        let log_path = LogFile::path(&self, &logger);
        if let Some(name) = log_path.file_name() {
            progress_manager
                .set_current_suffix(name.to_string_lossy().to_string());
        }

        let sybils: Vec<Uri> = self.attack.sybils().iter().copied().collect();

        let bars_per_exp = num_threads + 2;
        let start_bar = experiment_num * bars_per_exp;
        let coll_bar = start_bar + 1;
        let work_bar_start = start_bar + 2;

        // Pre-partition the action stream by device -> worker thread. Each
        // device is pinned to exactly one thread (LPT balancing above), so
        // every worker owns a disjoint set of devices and needs no cross-thread
        // synchronization while processing.
        let device_map = self.compute_device_assignments(num_threads);
        let mut partitions: Vec<Vec<(usize, &LazyAction)>> =
            vec![Vec::new(); num_threads];
        for (action_num, action) in self.dataset.actions().enumerate() {
            let thread_num = *device_map.get(&action.device_id()).expect(
                "Device ID from action not found in device_action_counts",
            );
            partitions[thread_num].push((action_num, action));
        }

        // Spawn one worker per partition; concat what they return. Workers may
        // finish in any order and touch no shared state.
        let mut all_reports: Vec<ReportItem> = Vec::new();
        let mut all_stats: Vec<DeviceStats> = Vec::new();

        thread::scope(|s| {
            let mut handles = vec![];
            for (i, partition) in partitions.into_iter().enumerate() {
                let self_ref = &self;
                let sybils = &sybils;
                let pm = progress_manager.clone();
                let bar_idx = work_bar_start + i;

                handles.push(s.spawn(move || {
                    self_ref.process_partition(&partition, sybils, &pm, bar_idx)
                }));
            }

            for handle in handles {
                let (reports, stats) =
                    handle.join().expect("Worker thread panicked");
                all_reports.extend(reports);
                all_stats.extend(stats);
            }
        });

        // THE determinism guarantee. Workers finish in arbitrary order, but
        // sorting every emitted report by (action_num, sub_index) reconstructs
        // the exact single-threaded dataset order. (action_num, sub_index) is a
        // total order over exactly the emitted reports. All budget/drop
        // decisions already happened inside each worker's compute_report, so
        // this reorder only affects the logger's (order-sensitive) metric
        // batching — which is what makes the output independent of thread
        // count.
        all_reports.sort_by_key(|r| (r.action_num, r.sub_index));

        progress_manager.init_experiment(
            coll_bar,
            all_reports.len(),
            "Collector".into(),
        );

        for item in all_reports {
            let logger_ctx = LoggerCtx {
                querier: item.querier,
                config: &self.config,
                conversion: &item.conversion,
                epoch_range: item.epoch_range,
            };
            logger.process_report(logger_ctx, item.report).unwrap();
        }

        logger.device_stats.extend(all_stats);
        progress_manager.finish_experiment(coll_bar);

        let logs_path = LogFile::path(&self, &logger);
        let logfile_contents = LogFile::build(&self, logger);

        // Spawn thread to save logs, so the caller can move on to the next
        // experiment while this one is being serialized to disk.
        let handle = thread::spawn(move || {
            logfile_contents.save(logs_path)?;
            progress_manager.finish_experiment(start_bar);
            Ok(())
        });

        Ok(handle)
    }
}

impl<'a, D: Dataset> Runtime for OnlineRuntime<'a, D> {
    type Dataset = D;
    type Attack = AnyAttack;

    fn dataset(&self) -> &Self::Dataset {
        self.dataset
    }

    fn attack(&self) -> &Self::Attack {
        &self.attack
    }

    fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    fn output_path(&self) -> PathBuf {
        let logger = (self.logger_factory)();
        LogFile::path(self, &logger)
    }
}
