//! Integration tests exercising `RecoveryPolicy` through a realistic
//! retry/recovery loop, as a caller of the crate would use it.

use agent_error_recovery::{ErrorClass, RecoveryAction, RecoveryPolicy};

/// Drive a transient error through repeated retries and confirm the policy
/// eventually escalates instead of looping forever.
#[test]
fn transient_error_eventually_escalates() {
    let mut policy = RecoveryPolicy::new(3);
    let mut attempt = 0;
    let last;

    // Simulate a tool that keeps timing out.
    loop {
        let action = policy.record("flaky_tool", ErrorClass::NetworkTimeout, attempt);
        if !action.is_retry() {
            last = action;
            break;
        }
        attempt += 1;
        // Safety net so a bug can't hang the test.
        assert!(attempt < 100, "policy never stopped retrying");
    }

    assert_eq!(last, RecoveryAction::Escalate);
    // attempts 0, 1, 2 retried; attempt 3 escalated => 4 recorded events.
    assert_eq!(policy.event_count_for("flaky_tool"), 4);
}

/// A non-retryable class should be acted on immediately without retrying.
#[test]
fn invalid_input_aborts_on_first_attempt() {
    let mut policy = RecoveryPolicy::default();
    let action = policy.record("search", ErrorClass::InvalidInput, 0);
    assert_eq!(action, RecoveryAction::Abort);
    assert!(!action.is_retry());
    assert_eq!(policy.events().len(), 1);
}

/// Per-class overrides must change the recommendation used during recording.
#[test]
fn override_changes_recorded_action() {
    let mut policy = RecoveryPolicy::new(5);
    policy.set(ErrorClass::NotFound, RecoveryAction::Abort);
    let action = policy.record("fetch", ErrorClass::NotFound, 0);
    assert_eq!(action, RecoveryAction::Abort);
}

/// Classify HTTP statuses and feed them through the policy end to end.
#[test]
fn classify_then_recommend_from_http_status() {
    let policy = RecoveryPolicy::default();

    let class = ErrorClass::from_status(429).expect("429 should classify");
    assert_eq!(class, ErrorClass::RateLimit);
    assert_eq!(policy.recommend(class), RecoveryAction::RetryWithBackoff);

    let class = ErrorClass::from_status(401).expect("401 should classify");
    assert_eq!(class, ErrorClass::AuthFailure);
    assert_eq!(policy.recommend(class), RecoveryAction::Escalate);

    // A successful status produces no error class.
    assert_eq!(ErrorClass::from_status(200), None);
}

/// Exponential backoff should be monotonic up to the clamp and never panic.
#[test]
fn backoff_schedule_is_monotonic_then_clamped() {
    let base = 50;
    let max = 1_000;
    let mut prev = 0;
    for attempt in 0..20u32 {
        let delay = RecoveryPolicy::backoff_ms(attempt, base, max);
        assert!(
            delay >= prev || delay == max,
            "backoff decreased unexpectedly"
        );
        assert!(delay <= max, "backoff exceeded max");
        prev = delay;
    }
    assert_eq!(RecoveryPolicy::backoff_ms(19, base, max), max);
}
