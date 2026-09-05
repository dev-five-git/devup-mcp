//! Spending Figma's read allowance at the rate it refills.
//!
//! Figma meters reads by the minute — ten of them on a Full seat of a
//! Professional plan — while a collection is a burst: one Section of three
//! page-height widths asks for some eight hundred nodes, dozens of calls fired
//! back to back. The burst crosses the ceiling within seconds of starting, and
//! because nothing slows it down, every wait afterwards is spent re-crossing
//! it. Idling first does not help, which is the observation that named this:
//! after five minutes of no calls at all the allowance is full, a two-call
//! request goes through, and a twenty-call one is refused exactly as before.
//! The allowance was never the scarce thing. The rate was.
//!
//! So the wait belongs before the call rather than after the refusal. Holding
//! each call until it fits under the ceiling turns a collection that could not
//! finish into one that merely takes longer — and for a snapshot being kept as
//! a test case, longer is the better trade.
//!
//! This does not replace waiting out a refusal. Other clients share the same
//! allowance, so the ceiling can be reached by calls this process never made,
//! and the retry above it still answers for that.

use std::{collections::VecDeque, sync::Mutex, time::Duration};

use tokio::time::{Instant, sleep};

/// The documented ceiling for the seat this is developed against: a Full seat
/// on a Professional plan, ten reads a minute. Organization allows fifteen and
/// Enterprise twenty, so the ceiling is read from the environment and defaults
/// only to the lowest of them — pacing too slowly costs time, pacing too fast
/// costs the collection.
const DEFAULT_CALLS_PER_MINUTE: usize = 10;

/// The period Figma meters over.
const WINDOW: Duration = Duration::from_secs(60);

/// Environment override, for a seat whose ceiling is higher than the default.
const LIMIT_VARIABLE: &str = "DEVUP_FIGMA_CALLS_PER_MINUTE";

/// Holds each read until it fits under a rolling per-minute ceiling.
pub struct CallPacer {
    limit: usize,
    window: Duration,
    /// When each call still inside the window was let through, oldest first.
    spent: Mutex<VecDeque<Instant>>,
}

impl CallPacer {
    pub fn from_env() -> Self {
        let limit = std::env::var(LIMIT_VARIABLE)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|limit| *limit > 0)
            .unwrap_or(DEFAULT_CALLS_PER_MINUTE);
        Self::new(limit, WINDOW)
    }

    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            spent: Mutex::new(VecDeque::new()),
        }
    }

    /// Returns once this call fits under the ceiling, counting it as spent.
    pub async fn acquire(&self) {
        while let Some(wait) = self.reserve() {
            sleep(wait).await;
        }
    }

    /// `None` when the call was recorded and may go ahead; otherwise how long
    /// until the oldest call leaves the window and a slot opens.
    ///
    /// Split out so the lock is released before the caller waits: holding it
    /// across the sleep would pace the calls one behind another rather than
    /// against the clock.
    fn reserve(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut spent = self.spent.lock().expect("call pacer");
        while spent
            .front()
            .is_some_and(|at| now.duration_since(*at) >= self.window)
        {
            spent.pop_front();
        }
        match spent.front() {
            Some(oldest) if spent.len() >= self.limit => {
                Some(self.window.saturating_sub(now.duration_since(*oldest)))
            }
            _ => {
                spent.push_back(now);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Up to the ceiling, nothing waits.
    #[tokio::test(start_paused = true)]
    async fn a_burst_under_the_ceiling_is_not_slowed() {
        let pacer = CallPacer::new(10, WINDOW);
        let start = Instant::now();
        for _ in 0..10 {
            pacer.acquire().await;
        }
        assert_eq!(Instant::now().duration_since(start), Duration::ZERO);
    }

    /// The eleventh has to wait for the first to leave the window, which is
    /// the whole window when the first ten went out together.
    #[tokio::test(start_paused = true)]
    async fn the_call_past_the_ceiling_waits_for_a_slot() {
        let pacer = CallPacer::new(10, WINDOW);
        let start = Instant::now();
        for _ in 0..11 {
            pacer.acquire().await;
        }
        assert_eq!(Instant::now().duration_since(start), WINDOW);
    }

    /// A collection the size of the one that prompted this — sixty calls at
    /// ten a minute — is spread across the windows it needs instead of being
    /// refused partway through.
    #[tokio::test(start_paused = true)]
    async fn a_collection_larger_than_the_allowance_is_spread_rather_than_refused() {
        let pacer = CallPacer::new(10, WINDOW);
        let start = Instant::now();
        for _ in 0..60 {
            pacer.acquire().await;
        }
        // Ten go at once and each further ten waits out a window, so the
        // sixtieth leaves five windows after the first.
        assert_eq!(Instant::now().duration_since(start), 5 * WINDOW);
    }

    /// A seat with a higher ceiling should not be paced to the lowest one.
    #[tokio::test(start_paused = true)]
    async fn a_raised_ceiling_lets_more_through_before_waiting() {
        let pacer = CallPacer::new(20, WINDOW);
        let start = Instant::now();
        for _ in 0..20 {
            pacer.acquire().await;
        }
        assert_eq!(Instant::now().duration_since(start), Duration::ZERO);
    }
}
