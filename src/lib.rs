/*!
agent-error-recovery: composable error recovery strategies for LLM agent tools.

```rust
use agent_error_recovery::{RecoveryPolicy, RecoveryAction, ErrorClass};

let policy = RecoveryPolicy::default();
let action = policy.recommend(ErrorClass::RateLimit);
assert_eq!(action, RecoveryAction::RetryWithBackoff);
```
*/

use std::fmt;

/// Error classification for recovery decisions.
///
/// Each variant represents a broad category of failure that an agent tool call
/// can produce. The [`RecoveryPolicy`] maps each class to a [`RecoveryAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// The tool was rate limited (e.g. HTTP 429). Usually transient.
    RateLimit,
    /// A network request timed out. Usually transient.
    NetworkTimeout,
    /// Authentication or authorization failed (e.g. HTTP 401/403).
    AuthFailure,
    /// The arguments supplied to the tool were invalid (e.g. HTTP 400).
    InvalidInput,
    /// The requested resource does not exist (e.g. HTTP 404).
    NotFound,
    /// The remote service returned an internal error (e.g. HTTP 5xx).
    ServerError,
    /// The error could not be classified.
    Unknown,
}

impl ErrorClass {
    /// Every [`ErrorClass`] variant, useful for iterating over all classes.
    pub const ALL: [ErrorClass; 7] = [
        ErrorClass::RateLimit,
        ErrorClass::NetworkTimeout,
        ErrorClass::AuthFailure,
        ErrorClass::InvalidInput,
        ErrorClass::NotFound,
        ErrorClass::ServerError,
        ErrorClass::Unknown,
    ];

    /// Best-effort classification of an HTTP status code into an [`ErrorClass`].
    ///
    /// Status codes below 400 are treated as success and therefore return
    /// `None`; callers should only invoke this for error responses.
    ///
    /// ```
    /// use agent_error_recovery::ErrorClass;
    ///
    /// assert_eq!(ErrorClass::from_status(429), Some(ErrorClass::RateLimit));
    /// assert_eq!(ErrorClass::from_status(404), Some(ErrorClass::NotFound));
    /// assert_eq!(ErrorClass::from_status(503), Some(ErrorClass::ServerError));
    /// assert_eq!(ErrorClass::from_status(200), None);
    /// ```
    pub fn from_status(status: u16) -> Option<ErrorClass> {
        match status {
            0..=399 => None,
            408 => Some(ErrorClass::NetworkTimeout),
            429 => Some(ErrorClass::RateLimit),
            401 | 403 => Some(ErrorClass::AuthFailure),
            404 => Some(ErrorClass::NotFound),
            400 | 405..=428 | 430..=499 => Some(ErrorClass::InvalidInput),
            500..=599 => Some(ErrorClass::ServerError),
            _ => Some(ErrorClass::Unknown),
        }
    }

    /// Whether errors of this class are typically transient and worth retrying.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ErrorClass::RateLimit | ErrorClass::NetworkTimeout | ErrorClass::ServerError
        )
    }
}

impl fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ErrorClass::RateLimit => "rate_limit",
            ErrorClass::NetworkTimeout => "network_timeout",
            ErrorClass::AuthFailure => "auth_failure",
            ErrorClass::InvalidInput => "invalid_input",
            ErrorClass::NotFound => "not_found",
            ErrorClass::ServerError => "server_error",
            ErrorClass::Unknown => "unknown",
        };
        write!(f, "{}", s)
    }
}

/// Recommended action for a recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryAction {
    /// Retry immediately.
    RetryImmediate,
    /// Retry after exponential backoff.
    RetryWithBackoff,
    /// Abort and report error to the LLM.
    Abort,
    /// Skip this tool call and continue.
    Skip,
    /// Escalate to human / supervisor.
    Escalate,
}

impl RecoveryAction {
    /// Whether this action asks the caller to retry the tool call.
    ///
    /// ```
    /// use agent_error_recovery::RecoveryAction;
    ///
    /// assert!(RecoveryAction::RetryImmediate.is_retry());
    /// assert!(RecoveryAction::RetryWithBackoff.is_retry());
    /// assert!(!RecoveryAction::Abort.is_retry());
    /// ```
    pub fn is_retry(&self) -> bool {
        matches!(
            self,
            RecoveryAction::RetryImmediate | RecoveryAction::RetryWithBackoff
        )
    }
}

impl fmt::Display for RecoveryAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            RecoveryAction::RetryImmediate => "retry_immediate",
            RecoveryAction::RetryWithBackoff => "retry_with_backoff",
            RecoveryAction::Abort => "abort",
            RecoveryAction::Skip => "skip",
            RecoveryAction::Escalate => "escalate",
        };
        write!(f, "{}", s)
    }
}

/// A recovery record in the log.
#[derive(Debug, Clone)]
pub struct RecoveryEvent {
    pub tool_name: String,
    pub error_class: ErrorClass,
    pub action: RecoveryAction,
    pub attempt: usize,
}

/// Policy that maps error classes to recovery actions, and keeps a log of the
/// recovery decisions it has made.
///
/// Construct one with [`RecoveryPolicy::default`] for sensible defaults, or
/// [`RecoveryPolicy::new`] to override the maximum number of attempts. Use
/// [`RecoveryPolicy::set`] to override the action for individual error classes.
pub struct RecoveryPolicy {
    /// Default action per error class.
    defaults: std::collections::HashMap<ErrorClass, RecoveryAction>,
    /// Max attempts before escalating.
    max_attempts: usize,
    /// Event log.
    events: Vec<RecoveryEvent>,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        use ErrorClass::*;
        use RecoveryAction::*;
        let mut defaults = std::collections::HashMap::new();
        defaults.insert(RateLimit, RetryWithBackoff);
        defaults.insert(NetworkTimeout, RetryWithBackoff);
        defaults.insert(AuthFailure, Escalate);
        defaults.insert(InvalidInput, Abort);
        defaults.insert(NotFound, Skip);
        defaults.insert(ServerError, RetryWithBackoff);
        defaults.insert(Unknown, Abort);
        Self {
            defaults,
            max_attempts: 3,
            events: Vec::new(),
        }
    }
}

impl RecoveryPolicy {
    /// Create a policy with default action mappings but a custom maximum number
    /// of attempts before [`record`](RecoveryPolicy::record) escalates.
    pub fn new(max_attempts: usize) -> Self {
        Self {
            max_attempts,
            ..Self::default()
        }
    }

    /// Override the action for a specific error class.
    pub fn set(&mut self, class: ErrorClass, action: RecoveryAction) {
        self.defaults.insert(class, action);
    }

    /// Recommend a recovery action for an error class.
    ///
    /// Classes without an explicit mapping fall back to
    /// [`RecoveryAction::Abort`].
    pub fn recommend(&self, class: ErrorClass) -> RecoveryAction {
        self.defaults
            .get(&class)
            .copied()
            .unwrap_or(RecoveryAction::Abort)
    }

    /// Record an event and return the recommended action.
    ///
    /// `attempt` is the zero-based index of the retry being considered. Once
    /// `attempt` reaches [`max_attempts`](RecoveryPolicy::max_attempts) the
    /// recommendation is forced to [`RecoveryAction::Escalate`] regardless of
    /// the class mapping, so retries cannot loop forever.
    pub fn record(&mut self, tool: &str, class: ErrorClass, attempt: usize) -> RecoveryAction {
        let action = if attempt >= self.max_attempts {
            RecoveryAction::Escalate
        } else {
            self.recommend(class)
        };
        self.events.push(RecoveryEvent {
            tool_name: tool.to_string(),
            error_class: class,
            action,
            attempt,
        });
        action
    }

    /// Compute an exponential backoff delay, in milliseconds, for a given
    /// attempt index.
    ///
    /// The delay grows as `base_ms * 2^attempt` and is clamped to `max_ms` so
    /// it never overflows or grows without bound. This is a pure helper: it does
    /// not sleep, leaving the choice of timer up to the caller.
    ///
    /// ```
    /// use agent_error_recovery::RecoveryPolicy;
    ///
    /// assert_eq!(RecoveryPolicy::backoff_ms(0, 100, 10_000), 100);
    /// assert_eq!(RecoveryPolicy::backoff_ms(1, 100, 10_000), 200);
    /// assert_eq!(RecoveryPolicy::backoff_ms(3, 100, 10_000), 800);
    /// // Clamped at the maximum.
    /// assert_eq!(RecoveryPolicy::backoff_ms(40, 100, 10_000), 10_000);
    /// ```
    pub fn backoff_ms(attempt: u32, base_ms: u64, max_ms: u64) -> u64 {
        let factor = 2u64.checked_pow(attempt);
        match factor {
            Some(f) => base_ms.saturating_mul(f).min(max_ms),
            None => max_ms,
        }
    }

    /// All recorded recovery events, oldest first.
    pub fn events(&self) -> &[RecoveryEvent] {
        &self.events
    }

    /// The maximum number of attempts before [`record`](RecoveryPolicy::record)
    /// escalates.
    pub fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    /// Clear the recorded event log.
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Count events recorded for a given tool.
    pub fn event_count_for(&self, tool: &str) -> usize {
        self.events.iter().filter(|e| e.tool_name == tool).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rate_limit_is_backoff() {
        let p = RecoveryPolicy::default();
        assert_eq!(
            p.recommend(ErrorClass::RateLimit),
            RecoveryAction::RetryWithBackoff
        );
    }

    #[test]
    fn default_auth_is_escalate() {
        let p = RecoveryPolicy::default();
        assert_eq!(
            p.recommend(ErrorClass::AuthFailure),
            RecoveryAction::Escalate
        );
    }

    #[test]
    fn default_invalid_input_is_abort() {
        let p = RecoveryPolicy::default();
        assert_eq!(p.recommend(ErrorClass::InvalidInput), RecoveryAction::Abort);
    }

    #[test]
    fn default_not_found_is_skip() {
        let p = RecoveryPolicy::default();
        assert_eq!(p.recommend(ErrorClass::NotFound), RecoveryAction::Skip);
    }

    #[test]
    fn override_policy() {
        let mut p = RecoveryPolicy::default();
        p.set(ErrorClass::InvalidInput, RecoveryAction::Skip);
        assert_eq!(p.recommend(ErrorClass::InvalidInput), RecoveryAction::Skip);
    }

    #[test]
    fn record_under_max() {
        let mut p = RecoveryPolicy::new(3);
        let action = p.record("tool_a", ErrorClass::RateLimit, 1);
        assert_eq!(action, RecoveryAction::RetryWithBackoff);
    }

    #[test]
    fn record_at_max_escalates() {
        let mut p = RecoveryPolicy::new(3);
        let action = p.record("tool_a", ErrorClass::RateLimit, 3);
        assert_eq!(action, RecoveryAction::Escalate);
    }

    #[test]
    fn events_logged() {
        let mut p = RecoveryPolicy::default();
        p.record("my_tool", ErrorClass::ServerError, 1);
        p.record("my_tool", ErrorClass::ServerError, 2);
        assert_eq!(p.events().len(), 2);
    }

    #[test]
    fn event_count_for_tool() {
        let mut p = RecoveryPolicy::default();
        p.record("a", ErrorClass::RateLimit, 0);
        p.record("b", ErrorClass::RateLimit, 0);
        p.record("a", ErrorClass::NetworkTimeout, 1);
        assert_eq!(p.event_count_for("a"), 2);
        assert_eq!(p.event_count_for("b"), 1);
    }

    #[test]
    fn clear_events() {
        let mut p = RecoveryPolicy::default();
        p.record("x", ErrorClass::Unknown, 0);
        p.clear_events();
        assert!(p.events().is_empty());
    }

    #[test]
    fn error_class_display() {
        assert_eq!(ErrorClass::RateLimit.to_string(), "rate_limit");
        assert_eq!(ErrorClass::AuthFailure.to_string(), "auth_failure");
    }

    #[test]
    fn recovery_action_display() {
        assert_eq!(
            RecoveryAction::RetryWithBackoff.to_string(),
            "retry_with_backoff"
        );
        assert_eq!(RecoveryAction::Abort.to_string(), "abort");
    }

    #[test]
    fn max_attempts_getter() {
        let p = RecoveryPolicy::new(5);
        assert_eq!(p.max_attempts(), 5);
    }

    #[test]
    fn new_preserves_default_mappings() {
        let p = RecoveryPolicy::new(7);
        assert_eq!(p.max_attempts(), 7);
        // Mappings from Default must survive the custom constructor.
        assert_eq!(
            p.recommend(ErrorClass::RateLimit),
            RecoveryAction::RetryWithBackoff
        );
        assert_eq!(
            p.recommend(ErrorClass::AuthFailure),
            RecoveryAction::Escalate
        );
    }

    #[test]
    fn from_status_maps_known_codes() {
        assert_eq!(ErrorClass::from_status(429), Some(ErrorClass::RateLimit));
        assert_eq!(
            ErrorClass::from_status(408),
            Some(ErrorClass::NetworkTimeout)
        );
        assert_eq!(ErrorClass::from_status(401), Some(ErrorClass::AuthFailure));
        assert_eq!(ErrorClass::from_status(403), Some(ErrorClass::AuthFailure));
        assert_eq!(ErrorClass::from_status(404), Some(ErrorClass::NotFound));
        assert_eq!(ErrorClass::from_status(400), Some(ErrorClass::InvalidInput));
        assert_eq!(ErrorClass::from_status(422), Some(ErrorClass::InvalidInput));
        assert_eq!(ErrorClass::from_status(500), Some(ErrorClass::ServerError));
        assert_eq!(ErrorClass::from_status(503), Some(ErrorClass::ServerError));
    }

    #[test]
    fn from_status_success_is_none() {
        assert_eq!(ErrorClass::from_status(200), None);
        assert_eq!(ErrorClass::from_status(204), None);
        assert_eq!(ErrorClass::from_status(301), None);
    }

    #[test]
    fn from_status_out_of_range_is_unknown() {
        assert_eq!(ErrorClass::from_status(600), Some(ErrorClass::Unknown));
        assert_eq!(ErrorClass::from_status(999), Some(ErrorClass::Unknown));
    }

    #[test]
    fn transient_classification() {
        assert!(ErrorClass::RateLimit.is_transient());
        assert!(ErrorClass::NetworkTimeout.is_transient());
        assert!(ErrorClass::ServerError.is_transient());
        assert!(!ErrorClass::AuthFailure.is_transient());
        assert!(!ErrorClass::InvalidInput.is_transient());
        assert!(!ErrorClass::NotFound.is_transient());
    }

    #[test]
    fn all_classes_constant_is_complete() {
        // Every class must have a default mapping and a Display string.
        let p = RecoveryPolicy::default();
        for class in ErrorClass::ALL {
            // recommend must not panic and Display must be non-empty.
            let _ = p.recommend(class);
            assert!(!class.to_string().is_empty());
        }
        assert_eq!(ErrorClass::ALL.len(), 7);
    }

    #[test]
    fn action_is_retry() {
        assert!(RecoveryAction::RetryImmediate.is_retry());
        assert!(RecoveryAction::RetryWithBackoff.is_retry());
        assert!(!RecoveryAction::Abort.is_retry());
        assert!(!RecoveryAction::Skip.is_retry());
        assert!(!RecoveryAction::Escalate.is_retry());
    }

    #[test]
    fn backoff_grows_exponentially() {
        assert_eq!(RecoveryPolicy::backoff_ms(0, 100, 10_000), 100);
        assert_eq!(RecoveryPolicy::backoff_ms(1, 100, 10_000), 200);
        assert_eq!(RecoveryPolicy::backoff_ms(2, 100, 10_000), 400);
        assert_eq!(RecoveryPolicy::backoff_ms(3, 100, 10_000), 800);
    }

    #[test]
    fn backoff_is_clamped_and_overflow_safe() {
        // Clamped to max once the doubling exceeds it.
        assert_eq!(RecoveryPolicy::backoff_ms(10, 100, 10_000), 10_000);
        // A huge attempt index must not panic or overflow.
        assert_eq!(RecoveryPolicy::backoff_ms(1000, 100, 10_000), 10_000);
        assert_eq!(RecoveryPolicy::backoff_ms(64, 1, u64::MAX), u64::MAX);
    }

    #[test]
    fn set_overrides_take_effect() {
        let mut p = RecoveryPolicy::default();
        assert_eq!(p.recommend(ErrorClass::Unknown), RecoveryAction::Abort);
        p.set(ErrorClass::Unknown, RecoveryAction::Skip);
        assert_eq!(p.recommend(ErrorClass::Unknown), RecoveryAction::Skip);
    }
}
