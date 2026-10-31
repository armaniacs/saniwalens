//! The headless heartbeat: the quiet-stretch logic for a logged headless run.
//!
//! A headless optimize can sit silent through a long tool call or a stretch of
//! model thinking, and silence reads as a hang. The heartbeat thread emits a
//! dim beat to stderr every [`CADENCE`] — elapsed seconds and how many events
//! have streamed — until the run's `stop` flag flips, so waiting is visible.
//! The loop lives here, apart from the spawn and the log, so the beat cadence
//! and the stop handling are pinned in one place (`docs/specs/harness.md`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use crate::session::Style;

/// How often a beat fires while the advisor works.
pub const CADENCE: Duration = Duration::from_secs(30);

/// How often the thread re-checks the `stop` flag between beats.
pub const POLL: Duration = Duration::from_millis(500);

/// Spawn the heartbeat thread: a dim beat to stderr every `cadence` while the
/// advisor works — elapsed seconds and how many events have streamed, so
/// silence reads as waiting, not a hang — until `stop` flips. Each beat also
/// arrives on the returned receiver, which disconnects the moment the thread
/// exits: the observation point that pins the cadence and the stop handling.
pub fn spawn(
    stop: Arc<AtomicBool>,
    events: Arc<AtomicUsize>,
    cadence: Duration,
    poll: Duration,
) -> (std::thread::JoinHandle<()>, Receiver<(u64, usize)>) {
    let (sender, receiver) = channel();
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let mut next = cadence;
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(poll);
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let elapsed = start.elapsed();
            if elapsed >= next {
                next += cadence;
                let secs = elapsed.as_secs();
                let count = events.load(Ordering::Relaxed);
                eprintln!(
                    "{}",
                    Style::stderr().dim(&format!("  … advisor working ({secs}s, {count} events)"))
                );
                let _ = sender.send((secs, count));
            }
        }
    });
    (handle, receiver)
}
