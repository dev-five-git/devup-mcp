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

use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use tokio::time::{Instant, sleep};

/// Under the documented ceiling, not at it. A Full seat on a Professional plan
/// is allowed ten reads a minute, and pacing to exactly ten failed: a capture
/// was refused after twenty-one calls because eight probe calls made moments
/// earlier were still inside the same window. The allowance is the account's,
/// not this collection's — an editor, another agent, or a second export draws
/// on the same ten — and Figma need not count a relayed call the way this side
/// counts it. Leaving two of the ten unspent buys room for both.
///
/// Organization allows fifteen and Enterprise twenty, so the ceiling is read
/// from the environment; the default is the lowest one, less its headroom.
const DEFAULT_CALLS_PER_MINUTE: usize = 8;

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

/// The window, whether or not a thread died holding it.
///
/// Poisoning says a thread unwound mid-update; it does not say the deque is
/// unreadable, and here it cannot be: every critical section below is a push,
/// a pop, a clear and some arithmetic on `Instant`, so the worst a half-done
/// update leaves is a window that counts one call wrong for at most a minute.
/// Taking the guard back is the whole recovery.
///
/// Unwrapping instead would spend that thread's panic twice. The first one
/// ends one call; the poison it leaves ends *every* later call, because each
/// one has to pass through this same lock before it can reach Figma — and a
/// server that answers nothing until it is restarted is a far worse failure
/// than the collection that was already lost. The cost of not doing that is
/// this line.
fn window<T>(result: Result<T, PoisonError<T>>) -> T {
    result.unwrap_or_else(PoisonError::into_inner)
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

    /// Records a refusal by filling the window, so the next call waits it out
    /// in full.
    ///
    /// A refusal means the ceiling was reached at a rate this pacer thought
    /// was safe, so the pacer's picture is the thing that is wrong and paying
    /// it back at the same rate only repeats the mistake. The first capture
    /// tried that: it retried into a full window for twenty-four minutes and
    /// spent some two hundred and forty calls to arrive at no result at all.
    /// Standing down for a whole window costs a minute and leaves the
    /// allowance to refill.
    pub fn penalise(&self) {
        let now = Instant::now();
        let mut spent = window(self.spent.lock());
        spent.clear();
        for _ in 0..self.limit {
            spent.push_back(now);
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
        let mut spent = window(self.spent.lock());
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

    /// The allowance belongs to the account rather than to one collection, so
    /// the default has to leave room for calls this pacer never saw. Pacing to
    /// exactly the documented ten is what let a capture be refused after
    /// twenty-one calls, so ten at the default rate must not all go out inside
    /// one window.
    #[tokio::test(start_paused = true)]
    async fn the_default_stays_under_the_ceiling_it_paces_against() {
        let pacer = CallPacer::new(DEFAULT_CALLS_PER_MINUTE, WINDOW);
        let start = Instant::now();
        for _ in 0..10 {
            pacer.acquire().await;
        }
        assert!(
            Instant::now().duration_since(start) > Duration::ZERO,
            "pacing at the ceiling leaves nothing for calls made elsewhere"
        );
    }

    /// A refusal means the picture was wrong, so the pacer stands down for a
    /// whole window rather than paying it back at the rate that was refused.
    #[tokio::test(start_paused = true)]
    async fn a_refusal_makes_the_next_call_wait_out_a_whole_window() {
        let pacer = CallPacer::new(10, WINDOW);
        pacer.acquire().await;
        pacer.penalise();

        let start = Instant::now();
        pacer.acquire().await;
        assert_eq!(Instant::now().duration_since(start), WINDOW);
    }

    /// A thread that dies holding the window must not take every later call
    /// with it.
    ///
    /// Every read passes through this one lock, so a poisoned window is not
    /// one failed call — it is the end of the process's ability to reach
    /// Figma at all, for as long as it runs. The deque is still readable
    /// after such a panic, so the pacer takes it back and keeps pacing.
    #[tokio::test(start_paused = true)]
    async fn a_window_poisoned_by_another_thread_still_paces() {
        let pacer = CallPacer::new(10, WINDOW);
        let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = window(pacer.spent.lock());
            panic!("a thread unwound while holding the window");
        }));
        assert!(died.is_err(), "the window has to actually be poisoned");
        assert!(pacer.spent.is_poisoned());

        // Both paths through the lock still work, and still pace: ten calls
        // fit, and the eleventh waits out the window rather than panicking.
        let start = Instant::now();
        for _ in 0..11 {
            pacer.acquire().await;
        }
        assert_eq!(Instant::now().duration_since(start), WINDOW);

        pacer.penalise();
        let after_penalty = Instant::now();
        pacer.acquire().await;
        assert_eq!(Instant::now().duration_since(after_penalty), WINDOW);
    }

    /// And it is a pause, not a shutdown: once the window has passed the pacer
    /// goes back to full speed.
    #[tokio::test(start_paused = true)]
    async fn the_pacer_recovers_its_full_rate_after_standing_down() {
        let pacer = CallPacer::new(10, WINDOW);
        pacer.penalise();
        pacer.acquire().await;

        let start = Instant::now();
        for _ in 0..9 {
            pacer.acquire().await;
        }
        assert_eq!(Instant::now().duration_since(start), Duration::ZERO);
    }
}
