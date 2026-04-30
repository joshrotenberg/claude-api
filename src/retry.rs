//! Retry policy honoring `Retry-After`.
//!
//! [`RetryPolicy`] decides *when* and *how long* to wait between attempts.
//! [`Error::is_retryable`](crate::error::Error::is_retryable) decides
//! *whether* a given error is worth retrying. The retry loop itself lives on
//! [`Client::execute_with_retry`](crate::client::Client) and combines the two.
//!
//! Defaults are conservative: 3 attempts, exponential backoff from 500 ms to
//! 30 s, full jitter, `Retry-After` honored. Replace via
//! [`ClientBuilder::retry`](crate::client::ClientBuilder::retry).

use std::time::Duration;

/// Jitter strategy applied to the computed backoff.
///
/// Reduces the risk that a thundering herd of clients all retry at the
/// exact same moment. See AWS's "Exponential backoff and jitter" post for
/// the underlying math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Jitter {
    /// No jitter; sleep for exactly the computed exponential backoff.
    None,
    /// Sleep for a random duration in `[0, backoff]`. Maximum smoothing.
    #[default]
    Full,
    /// Sleep for a random duration in `[backoff/2, backoff]`. Compromise
    /// between predictability and herd-avoidance.
    Equal,
}

impl Jitter {
    /// Apply this jitter strategy to a deterministic backoff value.
    pub fn apply(self, value: Duration) -> Duration {
        match self {
            Self::None => value,
            Self::Full => {
                let max_ms = u64::try_from(value.as_millis()).unwrap_or(u64::MAX);
                if max_ms == 0 {
                    return Duration::ZERO;
                }
                Duration::from_millis(pseudo_random_u64() % (max_ms + 1))
            }
            Self::Equal => {
                let total_ms = u64::try_from(value.as_millis()).unwrap_or(u64::MAX);
                let half = total_ms / 2;
                if half == 0 {
                    return value;
                }
                Duration::from_millis(half + (pseudo_random_u64() % (half + 1)))
            }
        }
    }
}

/// Retry policy applied to outbound requests.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Maximum total attempts (1 = no retries, 3 = up to 2 retries after the
    /// initial attempt).
    pub max_attempts: u32,
    /// Backoff before the second attempt; doubled each retry, capped at
    /// [`Self::max_backoff`].
    pub initial_backoff: Duration,
    /// Hard cap on the backoff between attempts.
    pub max_backoff: Duration,
    /// Jitter strategy.
    pub jitter: Jitter,
    /// If `true` and the server sent a `Retry-After` header, sleep at least
    /// that long (`max(jittered_backoff, retry_after)`).
    pub respect_retry_after: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            jitter: Jitter::Full,
            respect_retry_after: true,
        }
    }
}

impl RetryPolicy {
    /// A policy that disables retries entirely.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            jitter: Jitter::None,
            respect_retry_after: false,
        }
    }

    /// Compute the duration to wait *before* the next attempt.
    ///
    /// `attempt` is the count of failed attempts so far (1 = "first attempt
    /// failed; about to do the first retry"). `server_retry_after` is the
    /// `Retry-After` header value parsed by the client's response decoder.
    #[must_use]
    pub fn compute_backoff(&self, attempt: u32, server_retry_after: Option<Duration>) -> Duration {
        // Cap the shift to avoid overflow even with absurd attempt counts.
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1).min(30));
        let exponential = self
            .initial_backoff
            .saturating_mul(factor)
            .min(self.max_backoff);
        let jittered = self.jitter.apply(exponential);

        if self.respect_retry_after {
            if let Some(server) = server_retry_after {
                return jittered.max(server);
            }
        }
        jittered
    }
}

/// Cheap pseudo-random source for jitter. Not cryptographic; we use system
/// time nanoseconds as entropy. Sufficient for spreading retries across a
/// fleet of clients.
fn pseudo_random_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| {
        let nanos = d.as_nanos();
        // Mix high and low 64-bit halves so we get a bit more variability
        // when calls happen in quick succession. Truncation is intentional.
        #[allow(clippy::cast_possible_truncation)]
        let mixed = (nanos as u64) ^ ((nanos >> 64) as u64);
        mixed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn deterministic_policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_secs(1),
            jitter: Jitter::None,
            respect_retry_after: false,
        }
    }

    #[test]
    fn compute_backoff_grows_exponentially() {
        let p = deterministic_policy();
        assert_eq!(p.compute_backoff(1, None), Duration::from_millis(10));
        assert_eq!(p.compute_backoff(2, None), Duration::from_millis(20));
        assert_eq!(p.compute_backoff(3, None), Duration::from_millis(40));
        assert_eq!(p.compute_backoff(4, None), Duration::from_millis(80));
    }

    #[test]
    fn compute_backoff_caps_at_max() {
        let p = RetryPolicy {
            max_backoff: Duration::from_millis(50),
            ..deterministic_policy()
        };
        assert_eq!(p.compute_backoff(20, None), Duration::from_millis(50));
        assert_eq!(p.compute_backoff(100, None), Duration::from_millis(50));
    }

    #[test]
    fn respect_retry_after_uses_max_of_server_and_jittered() {
        let p = RetryPolicy {
            respect_retry_after: true,
            ..deterministic_policy()
        };
        // Server says 5s; our backoff at attempt 1 is 10ms; pick 5s.
        assert_eq!(
            p.compute_backoff(1, Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn respect_retry_after_false_ignores_server_header() {
        let p = deterministic_policy(); // respect_retry_after = false
        assert_eq!(
            p.compute_backoff(1, Some(Duration::from_secs(60))),
            Duration::from_millis(10)
        );
    }

    #[test]
    fn jitter_none_is_identity() {
        assert_eq!(
            Jitter::None.apply(Duration::from_millis(42)),
            Duration::from_millis(42)
        );
    }

    #[test]
    fn jitter_full_stays_within_range() {
        let max = Duration::from_millis(100);
        for _ in 0..50 {
            let v = Jitter::Full.apply(max);
            assert!(v <= max, "{v:?} should be <= {max:?}");
        }
    }

    #[test]
    fn jitter_equal_stays_in_upper_half() {
        let max = Duration::from_millis(100);
        for _ in 0..50 {
            let v = Jitter::Equal.apply(max);
            assert!(v >= Duration::from_millis(50), "{v:?} below half");
            assert!(v <= max, "{v:?} above max");
        }
    }

    #[test]
    fn none_policy_skips_retries() {
        let p = RetryPolicy::none();
        assert_eq!(p.max_attempts, 1);
        assert_eq!(p.initial_backoff, Duration::ZERO);
        assert!(!p.respect_retry_after);
    }

    #[test]
    fn default_policy_matches_spec() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.initial_backoff, Duration::from_millis(500));
        assert_eq!(p.max_backoff, Duration::from_secs(30));
        assert_eq!(p.jitter, Jitter::Full);
        assert!(p.respect_retry_after);
    }
}
