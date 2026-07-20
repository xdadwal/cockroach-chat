//! Injected time. The core never calls `SystemTime::now()`; callers pass milliseconds in.
//!
//! Native shells wrap the platform monotonic clock; the simulator advances a [`ManualClock`]
//! by hand so every test is deterministic and reproducible.

use std::cell::Cell;

/// Milliseconds since an arbitrary but fixed epoch (monotonic within a run).
pub type Millis = u64;

pub trait Clock {
    fn now_ms(&self) -> Millis;
}

/// A clock the caller advances explicitly. Used by tests and the simulator.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: Cell<Millis>,
}

impl ManualClock {
    pub fn new(start: Millis) -> Self {
        Self {
            now: Cell::new(start),
        }
    }

    /// Move time forward by `delta` ms and return the new value.
    pub fn advance(&self, delta: Millis) -> Millis {
        let next = self.now.get().saturating_add(delta);
        self.now.set(next);
        next
    }

    pub fn set(&self, value: Millis) {
        self.now.set(value);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> Millis {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(1000);
        assert_eq!(c.now_ms(), 1000);
        assert_eq!(c.advance(250), 1250);
        assert_eq!(c.now_ms(), 1250);
        c.set(42);
        assert_eq!(c.now_ms(), 42);
    }

    #[test]
    fn advance_saturates() {
        let c = ManualClock::new(u64::MAX - 1);
        assert_eq!(c.advance(100), u64::MAX);
    }
}
