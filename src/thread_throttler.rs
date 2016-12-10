use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// A ThreadThrottle is used to control the rate of
/// thread progress, for example to limit the number
/// of requests sent to a web API per second.
/// If too many threads try to get through the
/// ThreadThrottle at once, the additional ones
/// will be slept until some time has passed.
/// Note that the throttle will not necessarily let work
/// through in FIFO order.
pub struct ThreadThrottler {
    rate: u32,
    interval: Duration,

    action_history: Mutex<VecDeque<Instant>>,
    cv: Condvar,
}

impl ThreadThrottler {
    /// Creates a new thread throttle that will let the
    /// specified number of calls pass within the provided
    /// interval.
    pub fn new(rate: u32, interval: Duration) -> ThreadThrottler {
        assert!(rate > 0, "Rate must be positive.");
        assert!(interval > Duration::new(0, 0), "Duration must be non-zero.");

        let mut tt = ThreadThrottler {
            rate: rate,
            interval: interval,

            action_history: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
        };
        tt
    }

    /// Attempts to pass through the throttle. If there is
    /// sufficient capacity it will return immediately.
    /// Otherwise, the calling thread will block for some
    /// time before trying to pass through again.
    pub fn pass_through_or_block(&self) {
        let mut history = self.action_history.lock().unwrap();
        prune_history(&mut history, (Instant::now() - self.interval));

        while history.len() >= self.rate as usize {
            let minimum_sleep = (*history.get(0).unwrap() + self.interval) - Instant::now();
            let (lock_result, _) = self.cv.wait_timeout(history, minimum_sleep).unwrap();
            history = lock_result;
            prune_history(&mut history, (Instant::now() - self.interval));
        }

        history.push_back(Instant::now());
    }
}

/// Prunes a sorted history of events, cutting off those
/// older than a cutoff.
fn prune_history(history: &mut VecDeque<Instant>, cutoff: Instant) {
    if history.is_empty() {
        return;
    }

    while !history.is_empty() && *history.front().unwrap() < cutoff {
        history.pop_front();
    }
}


#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    #[should_panic(expected = "Rate must be positive")]
    fn test_bad_rate() {
        let tt = ThreadThrottler::new(0, Duration::new(0, 1));
    }

    #[test]
    fn test_basic_throttle() {
        // Let a thread through 1 time every 100 milliseconds.
        let tt = ThreadThrottler::new(1, Duration::new(0, 100_000_000));
        let start_time = Instant::now();
        for x in 0..11 {
            tt.pass_through_or_block();
        }
        let run_time = Instant::now() - start_time;
        assert!(run_time > Duration::new(1, 0));
    }
}
