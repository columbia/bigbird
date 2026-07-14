use std::path::PathBuf;

use pdslib::queries::ppa_histogram::RequestedBuckets;
use time::{OffsetDateTime, macros::format_description};

use crate::common_types::BucketKey;

pub fn minus_one_if_inf(x: f64) -> f64 {
    if x.is_infinite() { -1.0 } else { x }
}

/// Deterministic RNG seeded from per-action data. Seeding attack randomness
/// (sybil selection, genuine-first coin flips) this way makes the attacks
/// reproducible run-to-run and independent of worker-thread scheduling.
pub fn deterministic_rng(
    seed_parts: impl std::hash::Hash,
) -> rand::rngs::StdRng {
    use std::hash::Hasher as _;

    use rand::SeedableRng as _;

    let mut hasher = rustc_hash::FxHasher::default();
    seed_parts.hash(&mut hasher);
    rand::rngs::StdRng::seed_from_u64(hasher.finish())
}

pub fn repo_root_dir() -> PathBuf {
    // The `bigbirdeval` crate lives directly under the repository root, so the
    // parent of its manifest directory is the repo root (which contains
    // `data/`). Resolving via CARGO_MANIFEST_DIR keeps this robust regardless
    // of the current working directory or the name of the unpacked tarball
    // directory (e.g. `bigbird-sosp26/`).
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bigbirdeval crate should have a parent directory")
        .to_path_buf()
}

pub fn logs_dir() -> PathBuf {
    repo_root_dir().join("logs")
}

pub fn run_logs_dir(
    now: OffsetDateTime,
    exp_name: Option<&str>,
    output_dir: Option<&PathBuf>,
) -> PathBuf {
    if let Some(path) = output_dir {
        return path.clone();
    }
    match exp_name {
        Some(name) => logs_dir().join(format!("runtime_outputs_{name}")),
        None => {
            let now_str = now
                .format(format_description!(
                    "[month]_[day]_[hour]_[minute]_[second]"
                ))
                .unwrap();

            logs_dir().join(format!("runtime_outputs_{now_str}"))
        }
    }
}

pub fn init_logging() {
    log4rs::init_file("log4rs.yaml", Default::default())
        .expect("Failed to initialize logging");
}

pub fn buckets_vec(buckets: &RequestedBuckets<BucketKey>) -> Vec<BucketKey> {
    match buckets {
        RequestedBuckets::AllBuckets => (0..5).collect(),
        RequestedBuckets::SpecificBuckets(set) => set.iter().copied().collect(),
    }
}
