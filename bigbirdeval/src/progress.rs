use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use kdam::{Bar, BarExt};
use terminal_size::{Height, Width, terminal_size};

struct ExperimentState {
    count: AtomicUsize,
    total: AtomicUsize,
    desc: Mutex<Option<String>>,
    finished: AtomicBool,
}

struct SharedState {
    experiments: Vec<ExperimentState>,
    global_count: AtomicUsize,
    global_total: usize,
    running: AtomicBool,
    aborted: AtomicBool,
    // Info for display header
    header_info: String,
    current_suffix: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct ProgressManager {
    state: Arc<SharedState>,
}

impl ProgressManager {
    pub fn new(
        total_work: usize,
        num_experiments: usize,
        header_info: String,
    ) -> (Self, thread::JoinHandle<()>) {
        let mut experiments = Vec::with_capacity(num_experiments);
        for _ in 0..num_experiments {
            experiments.push(ExperimentState {
                count: AtomicUsize::new(0),
                total: AtomicUsize::new(0),
                desc: Mutex::new(None),
                finished: AtomicBool::new(false),
            })
        }

        let state = Arc::new(SharedState {
            experiments,
            global_count: AtomicUsize::new(0),
            global_total: total_work,
            running: AtomicBool::new(true),
            aborted: AtomicBool::new(false),
            header_info,
            current_suffix: Mutex::new(None),
        });

        let state_clone = state.clone();
        let handle = thread::spawn(move || run_monitor(state_clone));

        (Self { state }, handle)
    }

    pub fn init_experiment(&self, index: usize, total: usize, desc: String) {
        if let Some(exp) = self.state.experiments.get(index) {
            exp.total.store(total, Ordering::Relaxed);
            *exp.desc.lock().unwrap() = Some(desc);
            exp.count.store(0, Ordering::Relaxed);
            exp.finished.store(false, Ordering::Relaxed);
        }
    }

    pub fn update_add(&self, index: usize, n: usize, update_global: bool) {
        if let Some(exp) = self.state.experiments.get(index) {
            exp.count.fetch_add(n, Ordering::Relaxed);
        }
        if update_global {
            self.state.global_count.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub fn update_global(&self, n: usize) {
        self.state.global_count.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_total(&self, index: usize, n: usize) {
        if let Some(exp) = self.state.experiments.get(index) {
            exp.total.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub fn set_total(&self, index: usize, total: usize) {
        if let Some(exp) = self.state.experiments.get(index) {
            exp.total.store(total, Ordering::Relaxed);
        }
    }

    pub fn global_count(&self) -> usize {
        self.state.global_count.load(Ordering::Relaxed)
    }

    pub fn global_total(&self) -> usize {
        self.state.global_total
    }

    pub fn get_current(&self, index: usize) -> usize {
        if let Some(exp) = self.state.experiments.get(index) {
            exp.count.load(Ordering::Relaxed)
        } else {
            0
        }
    }

    pub fn finish_experiment(&self, index: usize) {
        if let Some(exp) = self.state.experiments.get(index) {
            exp.finished.store(true, Ordering::Relaxed);
        }
    }

    pub fn set_current_suffix(&self, suffix: String) {
        let mut lock = self.state.current_suffix.lock().unwrap();
        *lock = Some(suffix);
    }

    pub fn stop(&self) {
        self.state.running.store(false, Ordering::Relaxed);
    }

    pub fn abort(&self) {
        self.state.aborted.store(true, Ordering::Relaxed);
        self.state.running.store(false, Ordering::Relaxed);
    }
}

fn run_monitor(state: Arc<SharedState>) {
    let mut total_bar = kdam::tqdm!(
        total = state.global_total,
        desc = "Total progress",
        unit_scale = true,
        position = 0,
        force_refresh = true,
        dynamic_ncols = true
    );

    let mut exp_bars: Vec<Option<Bar>> =
        (0..state.experiments.len()).map(|_| None).collect();
    // Cache for last rendered lines to clear them correctly
    let mut last_printed_lines = 0;

    // Initial render
    // last_printed_lines = render(&mut total_bar, &mut exp_bars,
    // last_printed_lines);

    while state.running.load(Ordering::Relaxed) {
        let start_time = Instant::now();

        // 1. Update global bar
        let g_count = state.global_count.load(Ordering::Relaxed);
        let _ = total_bar.update_to(g_count);

        // 2. Manage experiment bars
        for (i, bar_opt) in exp_bars.iter_mut().enumerate() {
            let exp_state = &state.experiments[i];

            // If experiment is marked finished, ensure we clear the bar so it
            // disappears from UI
            if exp_state.finished.load(Ordering::Relaxed) {
                *bar_opt = None;
                continue;
            }

            // If bar doesn't exist yet, check if we should create it
            if bar_opt.is_none() {
                let desc_guard = exp_state.desc.lock().unwrap();
                if let Some(desc) = &*desc_guard {
                    let total = exp_state.total.load(Ordering::Relaxed);
                    let bar = kdam::tqdm!(
                        total = total,
                        desc = desc.clone(),
                        unit_scale = true,
                        dynamic_ncols = true
                    );
                    *bar_opt = Some(bar);
                }
            }

            // Sync bar with atomic counter
            if let Some(bar) = bar_opt {
                let count = exp_state.count.load(Ordering::Relaxed);

                // Sync total
                let current_total = exp_state.total.load(Ordering::Relaxed);
                if current_total != bar.total {
                    bar.total = current_total;
                }

                // Ensure count does not exceed total to prevent panics in kdam
                if bar.total > 0 && count > bar.total {
                    bar.total = count;
                }

                let _ = bar.update_to(count);
            }
        }

        // Calculate stats
        let total_exps = state.experiments.len();
        let finished_exps = state
            .experiments
            .iter()
            .filter(|e| e.finished.load(Ordering::Relaxed))
            .count();
        let running_exps = exp_bars.iter().filter(|b| b.is_some()).count();
        let queued_exps =
            total_exps.saturating_sub(finished_exps + running_exps);
        let pct_finished = if total_exps > 0 {
            (finished_exps as f64 / total_exps as f64) * 100.0
        } else {
            0.0
        };

        let header = format!(
            "Queued: {queued_exps} | Running: {running_exps} | Finished: {finished_exps} ({pct_finished:.1}%) | {hdr} ",
            hdr = state.header_info,
        );

        let suffix = state.current_suffix.lock().unwrap().clone();

        // 3. Render
        last_printed_lines = render(
            &mut total_bar,
            &mut exp_bars,
            last_printed_lines,
            &header,
            suffix,
        );

        // 4. Sleep ensuring ~10fps or less
        let elapsed = start_time.elapsed();
        if elapsed < Duration::from_millis(100) {
            thread::sleep(Duration::from_millis(100) - elapsed);
        }
    }

    // Final clear
    // We pass empty bars to clear everything except total? Or just clear
    // everything? User probably wants to see the final "Total Progress:
    // 100%". But individual experiments might be done.
    // Let's just clear the dynamic area.
    if !state.aborted.load(Ordering::Relaxed) {
        let _ = render(
            &mut total_bar,
            &mut vec![None; exp_bars.len()],
            last_printed_lines,
            &state.header_info,
            None,
        );
        println!(); // Print newline to preserve final output
    }
}

fn render(
    total_bar: &mut Bar,
    exp_bars: &mut [Option<Bar>],
    last_lines: usize,
    header_info: &str,
    suffix: Option<String>,
) -> usize {
    // Get terminal size
    let (Width(cols), Height(rows)) =
        terminal_size().unwrap_or((Width(80), Height(24)));
    let max_height = rows as usize;
    let cols = cols as usize;
    // Safety margin:
    // 1 line for top offset
    // 1 line for header
    // 1 line for total bar
    // 1 line for cursor/scroll buffer
    let safety_lines = if suffix.is_some() { 5 } else { 4 };

    if max_height <= safety_lines {
        return 0;
    }

    let mut lines_to_print = Vec::with_capacity(max_height);

    // Offset
    lines_to_print.push("".to_string());

    // 0. Header
    // Truncate safe for unicode
    let header_out: String = header_info.chars().take(cols).collect();
    lines_to_print.push(header_out);

    // 0.5 Suffix
    if let Some(s) = &suffix {
        let suffix_out: String =
            format!("Current: {s}").chars().take(cols).collect();
        lines_to_print.push(suffix_out);
    }

    // 1. Total Bar
    // Bar::render takes &mut self, returns String
    lines_to_print.push(total_bar.render().trim().to_string());

    // 2. Experiment bars
    // Calculate space
    let available_slots = max_height.saturating_sub(safety_lines);

    // Calculate max description length.
    // Use a fixed width to ensure alignment and prevent wrapping.
    // 40 chars should be plenty for "Worker X" or "Collector".
    let max_desc_len = 40;

    let mut slots_used = 0;
    for bar_opt in exp_bars.iter_mut() {
        if slots_used >= available_slots {
            break;
        }
        if let Some(bar) = bar_opt {
            let original_desc = bar.desc.clone();

            bar.desc =
                format!("{:<width$}", original_desc, width = max_desc_len);

            let s = bar.render();
            // Restore description (in case screen gets wider)
            bar.desc = original_desc;

            lines_to_print.push(s.trim().to_string());
            slots_used += 1;
        }
    }

    let mut handle = std::io::stderr().lock();

    // Clear previous output
    if last_lines > 0 {
        // Move up and to start of line
        let _ = write!(handle, "\x1b[{last_lines}A\x1b[1G"); // Move up
        let _ = write!(handle, "\x1b[J"); // Clear below
    }

    for line in &lines_to_print {
        let _ = writeln!(handle, "{line}");
    }
    let _ = handle.flush();

    lines_to_print.len()
}
