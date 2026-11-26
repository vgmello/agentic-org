//! Office-wide budgets for the unbound sign-in calls (`auth.*`, `TASK-137`).
//!
//! The per-connection budget ([`super::external::MAX_AUTH_CALLS_PER_CONNECTION`]) ends one
//! connection; a caller that opens a fresh connection per call is held by these token buckets,
//! shared by every connection of the office socket. Each kind of call has its own bucket, so a
//! flood of polls never starves a sign-in's challenge and the reverse:
//!
//! - `auth.login_request`: new pending logins (entropy, a directory lookup, a pending record);
//! - `auth.challenge`, `auth.exchange`, `auth.logout`: nonces and Ed25519 verifications;
//! - `auth.login_poll`: the waiting `agent-os login` processes, one poll a second each.
//!
//! A refused call answers `rate_limited` with `retry_after_ms`; nothing else is done for it.

use std::sync::Mutex;
use std::time::Instant;

use org_proto::ApiError;

/// One token bucket: up to `capacity` calls at once, refilled at `per_sec`.
struct Bucket {
    capacity: f64,
    per_sec: f64,
    state: Mutex<(f64, Option<Instant>)>,
}

impl Bucket {
    fn new((capacity, per_sec): (f64, f64)) -> Self {
        Self {
            capacity,
            per_sec,
            state: Mutex::new((capacity, None)),
        }
    }

    /// Takes one call at `now`, or answers how many milliseconds until one is free.
    fn take(&self, now: Instant) -> Result<(), u64> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let (tokens, last) = &mut *state;
        if let Some(last) = *last {
            let elapsed = now.saturating_duration_since(last).as_secs_f64();
            *tokens = (*tokens + elapsed * self.per_sec).min(self.capacity);
        }
        *last = Some(now);
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            return Ok(());
        }
        let wait_ms = ((1.0 - *tokens) / self.per_sec * 1000.0).ceil();
        // At most a few seconds by construction; the cast cannot overflow.
        Err(wait_ms.clamp(1.0, 60_000.0) as u64)
    }
}

/// Burst and refill per second of `auth.login_request`: a few sign-ins at once, then one
/// every 2 seconds.
pub(super) const LOGIN_REQUEST_BUDGET: (f64, f64) = (8.0, 0.5);
/// Burst and refill of `auth.challenge|exchange|logout` (two calls per token refresh).
pub(super) const SIGNED_BUDGET: (f64, f64) = (64.0, 16.0);
/// Burst and refill of `auth.login_poll`: room for every pending login polling each second.
pub(super) const POLL_BUDGET: (f64, f64) = (128.0, 48.0);

/// The office-wide sign-in budgets, one per kind of unbound `auth.*` call.
pub(crate) struct AuthRateLimits {
    login_request: Bucket,
    signed: Bucket,
    poll: Bucket,
}

impl Default for AuthRateLimits {
    fn default() -> Self {
        Self {
            login_request: Bucket::new(LOGIN_REQUEST_BUDGET),
            signed: Bucket::new(SIGNED_BUDGET),
            poll: Bucket::new(POLL_BUDGET),
        }
    }
}

impl AuthRateLimits {
    /// Admits one unbound `method` call at `now`, or answers the `rate_limited` refusal.
    /// A method that is no sign-in call is not counted here.
    pub(super) fn admit(&self, method: &str, now: Instant) -> Result<(), ApiError> {
        let bucket = match method {
            "auth.login_request" => &self.login_request,
            "auth.login_poll" => &self.poll,
            "auth.challenge" | "auth.exchange" | "auth.logout" => &self.signed,
            _ => return Ok(()),
        };
        bucket.take(now).map_err(|retry_after_ms| {
            let mut refusal = ApiError::rate_limited(retry_after_ms);
            refusal.message = "too many sign-in calls on this office; retry shortly".into();
            refusal
        })
    }
}

#[cfg(test)]
#[path = "auth_limits_tests.rs"]
mod tests;
