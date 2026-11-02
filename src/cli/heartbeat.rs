//! The headless heartbeat: the quiet-stretch logic for a logged headless run.
//!
//! A headless optimize can sit silent through a long tool call or a stretch of
//! model thinking, and silence reads as a hang. The heartbeat thread emits a
//! dim beat to stderr every [`CADENCE`] — elapsed seconds and how many events
//! have streamed — until the run's `stop` flag flips, so waiting is visible.
//! A beat missed while the thread is starved is dropped, not replayed: the
//! next deadline counts from the observation, so the resume beat is followed
//! by a full [`CADENCE`].
//! The loop lives here, apart from the spawn and the log, so the beat decision
//! and the stop handling live in one place (`docs/specs/harness.md`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use crate::session::Style;

/// How often a beat fires while the advisor works.
pub const CADENCE: Duration = Duration::from_secs(30);

/// How often the thread re-checks the `stop` flag between beats.
pub const POLL: Duration = Duration::from_millis(500);

/// The deadline for the beat after one observed at `elapsed`: one `cadence`
/// later, counted from the observation rather than from the previous deadline.
/// A stall therefore costs the slots it swallowed instead of replaying them
/// back-to-back when the loop resumes.
fn next_deadline(elapsed: Duration, cadence: Duration) -> Duration {
    elapsed.saturating_add(cadence)
}

/// Decide whether a beat is due at `elapsed` and, when it is, move `next` one
/// cadence past this observation. The spawn loop and the test's beat model
/// both drive this decision, so neither can drift from the other.
fn take_beat(elapsed: Duration, next: &mut Duration, cadence: Duration) -> bool {
    if elapsed < *next {
        return false;
    }
    *next = next_deadline(elapsed, cadence);
    true
}

/// Spawn the heartbeat thread: a dim beat to stderr every `cadence` while the
/// advisor works — elapsed seconds and how many events have streamed, so
/// silence reads as waiting, not a hang — until `stop` flips. The beats are a
/// sample sequence: the caller sees only the samples the receiver buffered,
/// not how many were produced or when. The returned receiver disconnects when
/// the thread exits, which is the stop the seam's test observes; a stopped
/// loop sends at most one more beat, a structural invariant of its two checks,
/// not an observed count.
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
            if take_beat(elapsed, &mut next, cadence) {
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc::RecvTimeoutError;

    use super::*;

    /// The beat timeline a poll loop produces for `samples`: one observation
    /// per poll, each one handed to the same decision the spawn loop uses.
    fn beats_on(samples: impl IntoIterator<Item = Duration>, cadence: Duration) -> Vec<Duration> {
        let mut next = cadence;
        let mut beats = Vec::new();
        for elapsed in samples {
            if take_beat(elapsed, &mut next, cadence) {
                beats.push(elapsed);
            }
        }
        beats
    }

    #[test]
    fn next_deadline_counts_from_the_observation() {
        let cadence = Duration::from_millis(30);
        assert_eq!(
            next_deadline(Duration::from_millis(210), cadence),
            Duration::from_millis(240),
            "a deadline six cadences behind is not replayed: the next beat is one cadence from now"
        );
        assert_eq!(next_deadline(Duration::ZERO, cadence), cadence);
        assert_eq!(
            next_deadline(Duration::MAX, cadence),
            Duration::MAX,
            "the deadline saturates instead of panicking at the Duration ceiling"
        );
    }

    #[test]
    fn consecutive_beats_stay_a_cadence_apart() {
        let cadence = Duration::from_millis(30);
        let mut samples: Vec<Duration> = (0..5).map(|i| Duration::from_millis(i * 5)).collect();
        // The thread freezes from 20ms to 210ms; the first poll after the
        // freeze is the observation far past the deadline.
        samples.extend(
            [210u64, 215, 220, 240, 245, 250, 255, 260, 270, 275, 290].map(Duration::from_millis),
        );

        let beats = beats_on(samples, cadence);
        assert_eq!(beats.len(), 3, "beats after the freeze: {beats:?}");
        for pair in beats.windows(2) {
            assert!(
                pair[1] - pair[0] >= cadence,
                "beat at {:?} came {:?} after {:?}, closer than the cadence",
                pair[1],
                pair[1] - pair[0],
                pair[0]
            );
        }
    }

    #[test]
    fn a_gap_longer_than_the_cadence_produces_one_beat() {
        let cadence = Duration::from_millis(30);
        let freeze = Duration::from_millis(600);
        let beats = beats_on(
            [
                Duration::from_millis(10),
                freeze,
                freeze + Duration::from_millis(5),
                freeze + Duration::from_millis(10),
            ],
            cadence,
        );
        assert_eq!(
            beats,
            vec![freeze],
            "a stalled beat lands once, not once per missed slot"
        );
    }

    #[test]
    fn a_stopped_heartbeat_sends_no_beat() {
        let stop = Arc::new(AtomicBool::new(true));
        let (handle, beats) = spawn(
            Arc::clone(&stop),
            Arc::new(AtomicUsize::new(0)),
            Duration::from_millis(5),
            Duration::from_millis(1),
        );
        assert_eq!(
            beats.recv_timeout(Duration::from_secs(2)),
            Err(RecvTimeoutError::Disconnected),
            "a stop flag set before the first poll ends the thread without a beat"
        );
        handle.join().unwrap();
    }
}
