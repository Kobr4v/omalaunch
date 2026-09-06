// SPDX-License-Identifier: GPL-3.0-or-later
//! Debounced, deduplicated operation queue.
//!
//! Rapid filesystem events coalesce: only the latest effective operation per
//! path survives, and execution waits `timeout` after the last event.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Integrate(PathBuf),
    Unintegrate(PathBuf),
}

impl Op {
    fn path(&self) -> &PathBuf {
        match self {
            Op::Integrate(p) | Op::Unintegrate(p) => p,
        }
    }

    fn is_integrate(&self) -> bool {
        matches!(self, Op::Integrate(_))
    }
}

pub struct Debouncer {
    ops: VecDeque<Op>,
    deadline: Option<Instant>,
    timeout: Duration,
}

impl Debouncer {
    pub fn new(timeout: Duration) -> Self {
        Self {
            ops: VecDeque::new(),
            deadline: None,
            timeout,
        }
    }

    pub fn pending(&self) -> usize {
        self.ops.len()
    }

    /// Queue `op` unless an identical effective operation is already pending.
    /// Scans from the back until an opposite action for the same path: a
    /// repeated action is a duplicate, an opposite action makes this new.
    pub fn schedule(&mut self, op: Op) {
        for existing in self.ops.iter().rev() {
            if existing.path() == op.path() {
                if existing.is_integrate() == op.is_integrate() {
                    return;
                }
                break;
            }
        }
        self.ops.push_back(op);
        self.deadline = Some(Instant::now() + self.timeout);
    }

    /// Drain the queue if the debounce deadline has passed.
    pub fn take_if_due(&mut self, now: Instant) -> Option<Vec<Op>> {
        match self.deadline {
            Some(d) if now >= d => Some(self.drain()),
            _ => None,
        }
    }

    /// Drain everything immediately (startup scan).
    pub fn drain(&mut self) -> Vec<Op> {
        self.deadline = None;
        self.ops.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn duplicates_coalesce() {
        let mut q = Debouncer::new(Duration::from_millis(10));
        q.schedule(Op::Integrate(p("/a.AppImage")));
        q.schedule(Op::Integrate(p("/a.AppImage")));
        assert_eq!(q.pending(), 1);
    }

    #[test]
    fn opposite_action_is_new() {
        let mut q = Debouncer::new(Duration::from_millis(10));
        q.schedule(Op::Integrate(p("/a.AppImage")));
        q.schedule(Op::Unintegrate(p("/a.AppImage")));
        assert_eq!(q.pending(), 2);
        // ...and a re-integrate after the unintegration is new again.
        q.schedule(Op::Integrate(p("/a.AppImage")));
        assert_eq!(q.pending(), 3);
    }

    #[test]
    fn unrelated_paths_independent() {
        let mut q = Debouncer::new(Duration::from_millis(10));
        q.schedule(Op::Integrate(p("/a.AppImage")));
        q.schedule(Op::Integrate(p("/b.AppImage")));
        assert_eq!(q.pending(), 2);
    }

    #[test]
    fn fires_after_timeout_only() {
        let mut q = Debouncer::new(Duration::from_millis(50));
        q.schedule(Op::Integrate(p("/a.AppImage")));
        assert!(q.take_if_due(Instant::now()).is_none());
        std::thread::sleep(Duration::from_millis(60));
        let ops = q.take_if_due(Instant::now()).expect("due");
        assert_eq!(ops.len(), 1);
        assert_eq!(q.pending(), 0);
    }

    #[test]
    fn daemon_survives_disappearing_dir() {
        // Scheduling for a path whose directory vanished must not panic;
        // execution filters by existence.
        let mut q = Debouncer::new(Duration::from_millis(1));
        q.schedule(Op::Integrate(p("/nonexistent-oma-dir/x.AppImage")));
        std::thread::sleep(Duration::from_millis(5));
        let ops = q.take_if_due(Instant::now()).expect("due");
        assert_eq!(ops.len(), 1);
    }
}
