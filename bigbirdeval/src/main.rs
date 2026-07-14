use std::sync::Mutex;

use clap::{Parser, ValueEnum};
use log::info;
use mimalloc::MiMalloc;
use time::OffsetDateTime;

use crate::{
    config::Runtime,
    datasets::{criteo::CriteoDataset, dataset_trait::Dataset},
    progress::ProgressManager,
    runtimes::online::OnlineRuntime,
    util::init_logging,
};

pub mod common_types;
pub mod config;
pub mod experiments;
pub mod progress;
pub mod querier;
pub mod uriset_localizer;
pub mod util;

pub mod runtimes {
    pub mod device_storage;
    pub mod online;
    pub mod stats_collector;
}
pub mod datasets {
    pub mod criteo;
    pub mod dataset_trait;
}
pub mod attacks {
    pub mod attack_b;
    pub mod attack_c;
    pub mod attack_trait;
    pub mod no_attack;
    pub mod scaffold;
}
pub mod logs {
    pub mod logfile;
    pub mod logger;
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, value_delimiter = ',')]
    experiment: Vec<ExperimentType>,

    #[arg(short, long, default_value_t = if cfg!(debug_assertions) { 1 } else { 32 })]
    num_threads: usize,

    #[arg(long)]
    exp_name: Option<String>,

    #[arg(long)]
    output_dir: Option<std::path::PathBuf>,

    #[arg(long, default_value_t = false)]
    force: bool,

    /// Cap the workload to the first N chronological actions. Smoke-test
    /// lever only: it exercises the pipeline end-to-end quickly and does NOT
    /// reproduce the paper's numbers. Omit for the full workload.
    #[arg(long)]
    max_items: Option<usize>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum ExperimentType {
    Paper,
    AttackerStrengthNumSybils,
    AttackerStrengthPopularity,
    BigBirdBc,
}

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> anyhow::Result<()> {
    init_logging();

    let args = Args::parse();
    let num_threads = args.num_threads;
    // let exp_name = args.exp_name;
    let output_dir = args.output_dir.as_ref();
    let force = args.force;
    let start = OffsetDateTime::now_local().unwrap();

    let mut experiments = vec![];

    for exp_type in &args.experiment {
        let suite_name =
            exp_type.to_possible_value().unwrap().get_name().to_string();

        let exps = match exp_type {
            ExperimentType::Paper => experiments::paper(),
            ExperimentType::AttackerStrengthNumSybils => {
                experiments::attacker_strength_num_sybils()
            }
            ExperimentType::AttackerStrengthPopularity => {
                experiments::attacker_strength_popularity()
            }
            ExperimentType::BigBirdBc => experiments::big_bird_bc(),
        };

        for exp in exps {
            experiments.push((suite_name.clone(), exp));
        }
    }

    let experiments_len = experiments.len();
    info!("Starting with {experiments_len} experiments");

    // take &ref so that the value can be moved into the threads
    let dataset = &CriteoDataset::new(args.max_items)?;

    let experiment_source = Mutex::new(experiments.into_iter().enumerate());

    let header_info = format!(
        "Threads: {num_threads} | Experiments: {:?}",
        args.experiment
    );

    let bars_per_exp = num_threads + 2; // distributor + collector + workers
    let ops_per_exp = dataset.len(); // Track only the main processing work

    let (progress_manager, monitor_handle) = ProgressManager::new(
        ops_per_exp * experiments_len,
        bars_per_exp * experiments_len,
        header_info,
    );

    let pm = progress_manager.clone();

    // set up panic hook, to avoid progress bars overwriting the panic message
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        pm.abort();
        std::thread::sleep(std::time::Duration::from_millis(200));
        default_hook(info);
    }));

    std::thread::scope(|s| {
        let mut threads = vec![];
        // for _thread_idx in 0..num_threads {
        let experiment_source = &experiment_source;
        let progress_manager = progress_manager.clone();

        let thread = s.spawn(move || {
            let mut save_threads = vec![];
            loop {
                let next_task = experiment_source.lock().unwrap().next();

                let (experiment_num, (suite_name, experiment)) = match next_task
                {
                    Some(task) => task,
                    None => break, // No more work
                };

                let task_output_dir =
                    output_dir.map(|p| p.join(&suite_name).join("logs"));
                if let Some(ref d) = task_output_dir {
                    std::fs::create_dir_all(d).ok();
                }

                let runtime = OnlineRuntime::new(
                    dataset,
                    experiment.attack_config,
                    experiment.runtime_config,
                    start,
                    Some(suite_name.clone()),
                    task_output_dir.clone(),
                );

                let path = runtime.output_path();
                if path.exists() && !force {
                    let bars_per_exp = num_threads + 2;
                    let start_bar = experiment_num * bars_per_exp;

                    // Update global progress
                    progress_manager.update_global(dataset.len());

                    for i in 0..bars_per_exp {
                        progress_manager.finish_experiment(start_bar + i);
                    }
                } else {
                    let handle = runtime.run(
                        experiment_num,
                        experiments_len,
                        num_threads,
                        // thread_idx,
                        progress_manager.clone(),
                    )?;
                    save_threads.push(handle);
                }
            }

            for handle in save_threads {
                handle
                    .join()
                    .expect("Save thread panicked")
                    .expect("Save thread failed");
            }

            dataset.cleanup_this_thread();

            Ok::<(), anyhow::Error>(())
        });

        threads.push(thread);
        // }

        for thread in threads {
            thread
                .join()
                .expect("Failed to join thread")
                .expect("Thread panicked");
        }
    });

    progress_manager.stop();
    monitor_handle.join().unwrap();

    println!();
    Ok(())
}
