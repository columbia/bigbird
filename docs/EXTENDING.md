# Extending Big Bird

This guide covers the two most common extensions to the evaluation engine:
adding a new **attack strategy** and adding a new **experiment suite**. All paths
are relative to the repo root.

The engine uses `enum_dispatch` for its trait polymorphism (e.g. the `AnyAttack`
/ `AnyAttackConfig` enums), so adding a variant is a small, mechanical change —
the compiler will flag every `match` you still need to extend.

## Adding a new attack

Attacks implement the `Attack` trait
(`bigbirdeval/src/attacks/attack_trait.rs`). They are stateless over `&self` and,
on each action, may inject synthetic malicious impressions/conversions into an
output buffer. Model your new attack on the two real strategies:

- `bigbirdeval/src/attacks/attack_b.rs` — the **Random** attacker.
- `bigbirdeval/src/attacks/attack_c.rs` — the **Omniscient** attacker.

Both reuse the shared targeting/injection helpers in
`bigbirdeval/src/attacks/scaffold.rs` (`Targeting`, `begin_attack`).

Steps:

1. **Create the module.** Add `bigbirdeval/src/attacks/attack_x.rs` defining two
   types: a `#[derive(Serialize, Debug, Clone, PartialEq, Hash)] AttackXConfig`
   (the swept parameters, serialized into each log file) and an `AttackX` struct
   holding `cfg`, its `queriers` map, and any state (e.g. a `sybils`
   `UriSetOrLocalizer`). Register the module in the `pub mod attacks { … }` block
   in `bigbirdeval/src/main.rs`.

2. **Implement `Attack` for `AttackX`.** The required methods are `file_suffix`,
   `process_action`, `querier` / `iter_queriers`, `config` (usually
   `serde_json::to_value(&self.cfg)`), and `attack_id`. Implement `sybils` and
   `cleanup_this_thread` if your attack uses them.

3. **Give it an explicit `attack_id()`.** Return a fresh integer distinct from the
   existing ids (`NoAttack → 0`, `AttackB → 2`, `AttackC → 3`). This id is what the
   `*_BY_attack_id_*` plots group curves on, so a unique value keeps your strategy
   as its own line in Fig. 6-style plots.

4. **Implement `AttackConfig` for `AttackXConfig`.** Provide
   `create_attack(&self, dataset) -> AnyAttack`, which builds the `AttackX` (and
   any queriers/sybils) and returns `this.into()`.

5. **Wire the enums** in `bigbirdeval/src/attacks/attack_trait.rs`: add
   `AttackX(AttackX)` to `enum AnyAttack` and `AttackXConfig(AttackXConfig)` to
   `enum AnyAttackConfig`, and add the `use` import at the top. `enum_dispatch`
   generates the dispatch and `From`/`Into` impls.

6. **If your attack takes `num_redirections`**, add its arm to the `match &mut
   attack_config` in `Experiment::new` (`bigbirdeval/src/config.rs`). That is where
   the redirection count is derived from the domain cap (`quota_count`) rather than
   set independently — an attack that ignores redirections needs no arm, but the
   `match` must still be exhaustive, so add `AttackXConfig(_) => {}` if nothing
   else.

## Adding a new experiment suite

A suite is a `Vec<Experiment>` — a set of `(RuntimeConfig, AnyAttackConfig)` points
swept over some knob. The existing factories in `bigbirdeval/src/experiments.rs`
(`paper`, `big_bird_bc`, `attacker_strength_num_sybils`,
`attacker_strength_popularity`) are the templates.

Steps:

1. **Add a factory** `pub fn my_suite() -> Vec<Experiment>` in
   `bigbirdeval/src/experiments.rs`. Build each point with `Experiment::new(runtime_config,
   attack_config.into_any())`; reuse the shared base configs / helpers the other
   suites use so defaults stay consistent.

2. **Register the CLI variant** in `bigbirdeval/src/main.rs`: add a variant to
   `enum ExperimentType` (e.g. `MySuite`) and a matching arm
   `ExperimentType::MySuite => experiments::my_suite()` in the `match exp_type`
   inside `main`. clap derives the kebab-case CLI name automatically, so it becomes
   selectable as `--experiment my-suite`.

3. **Register its figure** in `scripts/collect_figures.py` so
   `reproduce.sh`/`collect_figures.py` pick up the output. Add an entry to the
   `FIGURES` dict — `"figN_my_figure": ("my-suite", "<plot_stem>")` — where the stem
   is the filename (without extension) the plotting CLI writes into
   `<run>/<suite>/pdf/`. If your figure is a multi-panel composite like Fig. 6, add
   a panel tuple to `COMBINED_FIG6` instead.

Once registered, `scripts/run_experiments.py` will run your suite as part of a
full reproduction. To smoke-test it quickly, point `scripts/smoke.sh` at it by
editing its hardcoded `SUITE=` line (the script caps the workload via
`--max-items`).
