//! The office-wide sign-in budgets: bounded bursts, steady refill, separate kinds.

use std::time::{Duration, Instant};

use org_proto::ErrorCode;

use super::{AuthRateLimits, LOGIN_REQUEST_BUDGET, POLL_BUDGET, SIGNED_BUDGET};

fn burst((capacity, _): (f64, f64)) -> usize {
    capacity as usize
}

#[test]
fn each_kind_admits_its_burst_then_refuses_with_a_retry_hint() {
    let limits = AuthRateLimits::default();
    let now = Instant::now();
    for (method, budget) in [
        ("auth.login_request", LOGIN_REQUEST_BUDGET),
        ("auth.challenge", SIGNED_BUDGET),
        ("auth.login_poll", POLL_BUDGET),
    ] {
        for n in 0..burst(budget) {
            limits
                .admit(method, now)
                .unwrap_or_else(|e| panic!("{method} call {n}: {e:?}"));
        }
        let refused = limits.admit(method, now).unwrap_err();
        assert_eq!(refused.code, ErrorCode::RateLimited, "{method}");
        assert!(refused.retry_after_ms.is_some_and(|ms| ms >= 1), "{method}");
    }
    // Exchange and logout share the signed budget the challenges spent.
    assert!(limits.admit("auth.exchange", now).is_err());
    assert!(limits.admit("auth.logout", now).is_err());
    // Anything else is not a sign-in call and is never counted.
    for _ in 0..1_000 {
        limits.admit("auth.whoami", now).unwrap();
        limits.admit("msg.send", now).unwrap();
    }
}

#[test]
fn a_spent_kind_starves_no_other_and_refills_with_time() {
    let limits = AuthRateLimits::default();
    let now = Instant::now();
    while limits.admit("auth.login_poll", now).is_ok() {}
    // Polls are spent; a sign-in's challenge and a new login still go through.
    limits.admit("auth.challenge", now).unwrap();
    limits.admit("auth.login_request", now).unwrap();
    // A second later the polls have their refill back, bounded by the burst.
    let later = now + Duration::from_secs(1);
    let mut admitted = 0;
    while limits.admit("auth.login_poll", later).is_ok() {
        admitted += 1;
    }
    assert_eq!(admitted, POLL_BUDGET.1 as usize);
    let much_later = now + Duration::from_secs(3_600);
    let mut admitted = 0;
    while limits.admit("auth.login_poll", much_later).is_ok() {
        admitted += 1;
    }
    assert_eq!(admitted, burst(POLL_BUDGET), "never above the burst");
}

#[test]
fn login_requests_refill_at_one_every_two_seconds() {
    let limits = AuthRateLimits::default();
    let now = Instant::now();
    while limits.admit("auth.login_request", now).is_ok() {}
    let refused = limits.admit("auth.login_request", now).unwrap_err();
    assert!(refused.retry_after_ms.unwrap() <= 2_000);
    assert!(limits
        .admit("auth.login_request", now + Duration::from_millis(1_000))
        .is_err());
    limits
        .admit("auth.login_request", now + Duration::from_millis(2_001))
        .unwrap();
}
