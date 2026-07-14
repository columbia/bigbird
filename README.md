# Big Bird — SOSP 2026 Evaluation Artifact

Evaluation artifact for **Big Bird: Resilient Privacy Budgeting Across Untrusted
Web Domains** (SOSP 2026). Preprint: [arXiv:2506.05290](https://arxiv.org/abs/2506.05290).
It studies **sybil / budget-depletion attacks** against privacy-preserving
advertising systems built on a Private Data Service (PDS), and measures the
resulting privacy–utility tradeoffs on the Criteo ad dataset.

## Reproduce all paper figures

With **Docker** (recommended — pinned toolchain, no host setup beyond Docker):

```bash
git clone https://github.com/columbia/bigbird
cd bigbird
git submodule update --init pdslib      # the DP library the engine builds against
docker compose up --build
```

Figures are written to `figures/` as `fig5a` … `fig5d` and the combined
`fig6_attack_resilience.pdf`, renamed to the paper's exact figure names. The
container builds the Rust engine, runs the four paper suites over the Criteo
dataset, and plots them.

> **The first run downloads the ~169 MB Criteo dataset** into `data/` (skipped on
> re-runs) and is **compute-heavy**: each suite makes sequential passes over the
> full ~1.4M-device workload, so expect a few hours on a many-core server. Cap
> worker threads with `BB_THREADS=8 docker compose up --build`. Output files are
> written by the container (root-owned by Docker convention).

> Only `pdslib` is needed to build. The `firefox-prototype/` submodules are
> intentionally **not** initialized (they include a ~913 MB Firefox tree) — see
> [`firefox-prototype/README.md`](firefox-prototype/README.md) if you want that
> optional demo. Avoid a blanket `git submodule update --init --recursive`, which
> would pull them too.

<details>
<summary><b>Without Docker</b> (native, requires Rust + <code>uv</code>)</summary>

```bash
git clone https://github.com/columbia/bigbird
cd bigbird
git submodule update --init pdslib
./reproduce.sh
```

`reproduce.sh` runs the identical pipeline natively: `uv sync`, build the Criteo
dataset (skipped if `data/criteo/` exists), `cargo build --release`, run the four
suites, then collect the figures. `BB_THREADS=8 ./reproduce.sh` caps threads (the
orchestrator also retries a suite with fewer threads if it is OOM-killed).
</details>

## Quick smoke test

Before committing to a multi-hour full reproduction, run the smoke test to
confirm the whole pipeline works end-to-end (**build → run engine → plot**) in
seconds:

```bash
scripts/smoke.sh
```

Run it from anywhere — it resolves the repo root itself. It builds the release
engine, runs one small suite (`attacker-strength-popularity`, 10 runs) with the
Criteo workload **capped** to the first 300k chronological actions via the
engine's `--max-items` flag, plots a figure, and writes everything to a fresh
temporary directory (`mktemp -d` under `/tmp`) — never `figures/`, so it can
never clobber the real paper figures.

On an already-built binary the whole run takes **~14 s** (a cold
`cargo build --release` adds ~20 s, so worst case ~35 s). It prints a pass/fail
banner and points you at the example PDF and logs it produced.

> **This validates the pipeline; it does NOT reproduce the paper's numbers.** The
> capped run is a different, much smaller experiment. The real figures come from
> `./reproduce.sh` (see above).

Knobs (all optional):

- **first arg** — an explicit output dir (default: a fresh temp dir).
- `BB_SMOKE_MAX_ITEMS` — the workload cap (default `300000`; the engine keeps at
  least one conversion regardless).
- `BB_SMOKE_THREADS` — worker threads (default `nproc`).

## Determinism & reproducibility

The Criteo subsample is regenerated from the public dataset with a fixed seed
(`seed=0`), so the workload is identical on any machine. Given that workload the
Rust engine is deterministic and independent of `--num-threads` — reports are
reordered to the single-threaded sequence before any metric is computed — so
every figure is reproducible run-to-run.

The paper's figures were produced from an earlier, unseeded subsample of the
same dataset. The seeded reproduction is therefore **visually equivalent** to
the paper (identical curves and conclusions), with individual percentile markers
differing by at most a couple percent from the different random draw (Fig. 5
SSIM 0.92–1.00). The per-suite `malicious_*` outputs under `experiments/` — the
attacker's own measurement error, not shown in any paper figure — are diagnostic
only and are not part of this guarantee.

## What's here

| Component                     | Role in the paper                                                        |
| ----------------------------- | ------------------------------------------------------------------------ |
| `bigbirdeval/` (Rust)         | The evaluation engine `bigbirdeval` — runtimes, datasets, and the Random/Omniscient attacks (`bigbirdeval/src/attacks/`). |
| `bigbird/` (Python)           | Dataset preprocessing (`bigbird/datasets/criteo/`) and plotting (`bigbird/plots/`). |
| `pdslib/` (git submodule)     | The differential-privacy mechanism being evaluated (Apache-2.0, pinned commit). |
| `scripts/run_experiments.py`  | Orchestrator — builds a reproducible snapshot, runs every suite, plots.   |
| `scripts/collect_figures.py`  | Renames per-suite plot PDFs into paper figure names under `figures/`.     |
| `reproduce.sh`                | The single end-to-end entry point (calls the two scripts above).          |
| `firefox-prototype/`          | Optional: pdslib deployed in a modified Firefox + a dashboard extension (deployability demo, **not** needed for the figures). See [`firefox-prototype/README.md`](firefox-prototype/README.md). |

## Figure → experiment map

The system under evaluation is **Big Bird**. The paper evaluates two attacker
strategies: **Random** (Attack B) and **Omniscient** (Attack C). Fig. 6 is a
single three-panel figure assembled from three suites.

| Paper figure                          | Suite                            | How it's produced                                   |
| ------------------------------------- | -------------------------------- | --------------------------------------------------- |
| Fig. 5a — benign error, no attack     | `paper`                          | `no_attack_eps_qimp_p50_95`                          |
| Fig. 5b — error causes, no attack     | `paper`                          | `no_attack_bb_causes_eps_qimp`                       |
| Fig. 5c — benign error, under attack  | `paper`                          | `benign_attacked_eps_qimp_p50_95`                    |
| Fig. 5d — error causes, under attack  | `paper`                          | `benign_attacked_bb_causes_eps_qimp`                 |
| Fig. 6a — domain cap                  | `big-bird-bc`                     | `benign_attacked_quota_count_BY_attack_id_p50_95`    |
| Fig. 6b — sybil domains               | `attacker-strength-num-sybils`   | `benign_attacked_num_sybils_BY_attack_id_p50_95`     |
| Fig. 6c — site popularity             | `attacker-strength-popularity`   | `benign_attacked_malicious_site_rank_start_BY_attack_id_p50_95` |
| Table 3 — filter-config percentiles   | —                                | Manual, see [Manual steps](#manual-steps).           |

Fig. 5's four panels are collected as-is; Fig. 6's three panels are combined
into `figures/fig6_attack_resilience.pdf` by `bigbird/plots/combined.py`. The
authoritative `paper_name → (suite, stem)` map is the `FIGURES` dict (plus
`COMBINED_FIG6`) in `scripts/collect_figures.py`.

**Claim each figure supports** (paraphrased from §7 of the paper; the parenthetical
is the knob swept on the x-axis):

- **Fig. 5a** (source quota `q_imp`, no attack) — a correctly sized impression-site
  quota costs no benign utility: once `q_imp ≥ 2` Big Bird's median error matches
  the quota-less PPA baselines (5.2%), whereas an over-aggressive `q_imp = 0.5`
  inflates error 2.7× (to 14.0%) via null reports. The p85 quotas suffice for
  normal operation.
- **Fig. 5b** (source quota `q_imp`, no attack) — that residual error is fully
  explained by budget/quota blocking: at `q_imp = 0.5` the source quota blocks
  16.2% of reports, but for `q_imp ≥ 2` it blocks <0.002%, leaving only the ~2.8%
  per-querier blocking inherent to all IDP baselines. Sized to p85, Big Bird adds
  no bias beyond the underlying PPA accounting.
- **Fig. 5c** (source quota `q_imp`, under random attacker) — the quota mechanism
  keeps benign error near baseline under attack (median 5.5%, p95 14.8%) across a
  range `q_imp ∈ [1,2]`, while a bare global budget (PPA w/ global budget) is
  trivially defeated (median 48.5%, a 9.3× increase). There is a sweet spot:
  quotas that are too large also hurt, letting the attacker drain the global
  budget (≈20% error by `q_imp = 5`).
- **Fig. 5d** (source quota `q_imp`, under random attacker) — that degradation at
  large quotas is attributable specifically to global-budget depletion of benign
  reports: <1% blocked for `q_imp ≤ 2`, rising to ≈11% at `q_imp = 3` and ≈19% at
  `q_imp = 4`, where it dominates all other error sources.
- **Fig. 6a** (domain cap `q`, Random vs Omniscient) — the per-action domain cap
  bounds depletion even against the *optimal* omniscient attacker (`q = 1` keeps
  median benign error ≤10%, `q = 2` ≤28.6%, both far below the unprotected PPA
  API's 48.3–48.4%); atomic deduction further slashes damage against the realistic
  random attacker (5.4% vs the omniscient 28.6% at `q = 2`).
- **Fig. 6b** (# Sybil domains, Random vs Omniscient) — damage does not grow
  without bound in the size of the Sybil pool: the omniscient attacker saturates
  by ~10 domains (29.4%, flat through 100) and the random attacker barely moves
  (5.2%→6.4%), confirming resilience Thm. (i) that depletion is bounded by the
  product of domain count and quota capacity.
- **Fig. 6c** (attacker site rank, Random vs Omniscient) — damage scales with the
  volume of *genuine* user traffic the attacker attracts: as the attacker controls
  less-popular sites (top-10 → top-41–50) the omniscient attacker's error falls
  monotonically 29.0%→10.9% (2.7×) while the random attacker stays at baseline,
  confirming resilience Thm. (ii–iii). Attracting real traffic requires costly
  real-world investment, unlike registering cheap Sybil domains.

## Requirements / environment

- **Rust** — stable toolchain, edition 2024 (requires **rustc ≥ 1.85**); pinned in
  `rust-toolchain.toml`. Nightly is needed **only** for `just format` (`cargo +nightly fmt`).
- **Python 3.12+** via [`uv`](https://docs.astral.sh/uv/) — `uv sync` creates the
  venv and installs all deps plus tooling (`just`, `marimo`).
- **`pdslib` submodule** — a Cargo path dependency
  (`pdslib = { path = "../pdslib", features = ["fxhash", "experimental"] }`,
  `bigbirdeval/Cargo.toml`). The `experimental` feature is required for the report
  fields the RMSRE/bias metrics use. `git submodule update --init pdslib` checks
  it out at the pinned commit. (Init *only* `pdslib`, not `--recursive` — the
  `firefox-prototype/` submodules are large and optional.)
- **Dataset** — ~169 MB of preprocessed Criteo parquet under `data/criteo/`, built
  by `reproduce.sh` (or `just generate-dataset`) from the
  [CriteoPrivateAd](https://huggingface.co/datasets/criteo/CriteoPrivateAd) dataset
  (CC-BY-SA-4.0); needs network access to HuggingFace on first build.

**Hardware / OS used for the paper runs:**

- **OS:** Ubuntu 26.04 LTS (Linux kernel 7.0, x86-64)
- **CPU:** AMD EPYC 9B45 — 24 physical cores / 48 threads, single socket
- **RAM:** 182 GiB
- **Disk:** ~5 GB free (the ~169 MB dataset plus per-run snapshots under `experiments/`)

The engine is multi-threaded (`--num-threads`, or `BB_THREADS` when driving the
reproduction) and memory-heavy; peak RAM scales with thread count, and the four
suites run sequentially over the full dataset. On the machine above, a full
reproduction with 48 threads takes **roughly 25–30 minutes** of wall-clock
(plus the one-time image/dataset build on a cold start).

## Security note

This artifact **simulates** the sybil / budget-depletion attacks (Attacks A, B, C)
from the paper's threat model. It constructs synthetic sybil identities and
**injects synthetic malicious impressions and conversions entirely in-process**,
against a **local** copy of the Criteo dataset, purely to measure their effect on a
privacy budget. It **contacts and attacks no external system**, contains **no
exploit or malware**, and performs no networking of any kind. Identifiers such as
`malicious`, `sybil`, and `hijack` in the source refer only to these simulated
dataset entities. It is safe to build and run.

## Manual steps

**Table 3 (filter-config percentiles)** is not emitted by the figure pipeline; it
comes from the marimo notebook `notebooks/criteo_stats.py`, which reports the
per-user impression/conversion percentiles used to set the filter configs:

```bash
uv run marimo edit notebooks    # then open criteo_stats.py
# or run it headless:
uv run marimo run notebooks/criteo_stats.py
```

## Extending the artifact

To go beyond the shipped figures — adding a new attack strategy or a new
experiment suite — see [`docs/EXTENDING.md`](docs/EXTENDING.md). It walks through
the exact traits, enums, and match arms to touch (`Attack` in
`bigbirdeval/src/attacks/`, the suite factories in `bigbirdeval/src/experiments.rs`,
and figure registration in `scripts/collect_figures.py`).

## License

Apache-2.0 (see [`LICENSE`](LICENSE)); the `pdslib` submodule is also Apache-2.0.
The downloaded Criteo dataset is CC-BY-SA-4.0 (attribution + ShareAlike).
