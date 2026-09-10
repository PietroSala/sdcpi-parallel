//! The anytime buffer: every apex, with the instant it was recorded.
//!
//! A run of the frontier writes its result once, at the end, so a run killed
//! before it finishes leaves nothing. The buffer is the remedy: one line per
//! apex, appended and flushed as it enters, so that a process killed at any
//! moment leaves on disk everything it had found. The line carries the exact
//! value and the ticks, which is what the pruning at the end compares, so the
//! frontier at any instant can be rebuilt from the prefix of the buffer up to
//! that instant.
//!
//! Time is counted from the start of the process, not from the start of the
//! search: the parsing of the instance and the building of the grid are paid
//! inside the budget, as the building of the model is paid inside the budget
//! of the tools we are compared against. The line `search` marks where the
//! search proper begins, so the other origin is available too.
//!
//! The last line is `end`, with the time the search took and its status. Its
//! absence is the mark of a run that was killed.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use num_bigint::BigUint;
use num_rational::BigRational;

/// The start of the process, taken once by `main` before anything else.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Fixes the origin of time; the first call wins, the later ones are ignored.
pub fn mark_origin() {
    let _ = ORIGIN.set(Instant::now());
}

/// The origin of time, or the instant of this call when `main` never set one.
pub fn origin() -> Instant {
    *ORIGIN.get_or_init(Instant::now)
}

/// What a run publishes about its progress while it runs.
pub trait Progress: Send + Sync {
    /// The apexes recorded so far, the reads of the oracle and its prunings.
    fn snapshot(&self) -> (usize, usize, usize);
}

pub struct Sink {
    out: Mutex<BufWriter<File>>,
}

fn secs() -> f64 {
    origin().elapsed().as_secs_f64()
}

impl Sink {
    /// Creates the buffer, truncating what was there: one run, one file.
    pub fn create(path: &Path) -> Result<Sink, String> {
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Sink { out: Mutex::new(BufWriter::new(file)) })
    }

    /// Appends one line and pushes it to the kernel. A write that fails is
    /// dropped in silence: the buffer is a record of a run, and losing it must
    /// not lose the run.
    fn line(&self, text: &str) {
        if let Ok(mut out) = self.out.lock() {
            let _ = out.write_all(text.as_bytes());
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
    }

    pub fn start(&self, workers: usize) {
        self.line(&format!(
            "{{\"t\": {:.6}, \"start\": true, \"workers\": {}}}",
            secs(),
            workers
        ));
    }

    /// The instant the search proper begins, the grid being built by then.
    pub fn search(&self, components: &[usize]) {
        let comps: Vec<String> = components.iter().map(|c| c.to_string()).collect();
        self.line(&format!(
            "{{\"t\": {:.6}, \"search\": true, \"components\": [{}]}}",
            secs(),
            comps.join(", ")
        ));
    }

    /// One apex, as it enters the shared record. `n` is its index there, the
    /// number the pruning at the end uses to break a tie between equal ticks.
    pub fn point(
        &self,
        n: usize,
        value: &[BigRational],
        ticks: &[BigUint],
        decisions: usize,
        histories: usize,
        open: usize,
    ) {
        let v: Vec<String> = value.iter().map(|x| format!("\"{x}\"")).collect();
        let k: Vec<String> = ticks.iter().map(|x| x.to_string()).collect();
        self.line(&format!(
            "{{\"t\": {:.6}, \"n\": {}, \"v\": [{}], \"k\": [{}], \"d\": {}, \"h\": {}, \"o\": {}}}",
            secs(),
            n,
            v.join(", "),
            k.join(", "),
            decisions,
            histories,
            open
        ));
    }

    /// The counters, once a second, so that a killed run keeps them.
    pub fn stat(&self, apexes: usize, reads: usize, pruned: usize) {
        self.line(&format!(
            "{{\"t\": {:.6}, \"stat\": {{\"apexes\": {}, \"oracle_reads\": {}, \"oracle_pruned\": {}}}}}",
            secs(),
            apexes,
            reads,
            pruned
        ));
    }

    /// The end of the search, which a killed run never reaches.
    pub fn end(&self, status: &str, apexes: usize, search_seconds: f64) {
        self.line(&format!(
            "{{\"t\": {:.6}, \"end\": true, \"status\": \"{}\", \"apexes\": {}, \"search_seconds\": {:.6}}}",
            secs(),
            status,
            apexes,
            search_seconds
        ));
    }

    /// The frontier after the pruning, which says how many of the apexes
    /// survived: the check that a reader of the buffer rebuilds the same set.
    pub fn minimal(&self, points: usize) {
        self.line(&format!(
            "{{\"t\": {:.6}, \"minimal\": true, \"points\": {}}}",
            secs(),
            points
        ));
    }
}
