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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    RateLimit,
    NetworkTimeout,
    AuthFailure,
    InvalidInput,
    NotFound,
    ServerError,
    Unknown,
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
#[derive(Debug, Clone, PartialEq)]
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

/// Policy that maps error classes to recovery actions, with per-tool overrides.
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
        Self { defaults, max_attempts: 3, events: Vec::new() }
    }
}

impl RecoveryPolicy {
    pub fn new(max_attempts: usize) -> Self {
        let mut s = Self::default();
        s.max_attempts = max_attempts;
        s
    }

    /// Override the action for a specific error class.
    pub fn set(&mut self, class: ErrorClass, action: RecoveryAction) {
        self.defaults.insert(class, action);
    }

    /// Recommend a recovery action for an error class.
    pub fn recommend(&self, class: ErrorClass) -> RecoveryAction {
        self.defaults.get(&class).cloned().unwrap_or(RecoveryAction::Abort)
    }

    /// Record an event and return the recommended action (escalate if max attempts hit).
    pub fn record(&mut self, tool: &str, class: ErrorClass, attempt: usize) -> RecoveryAction {
        let action = if attempt >= self.max_attempts {
            RecoveryAction::Escalate
        } else {
            self.recommend(class.clone())
        };
        self.events.push(RecoveryEvent { tool_name: tool.to_string(), error_class: class, action: action.clone(), attempt });
        action
    }

    pub fn events(&self) -> &[RecoveryEvent] { &self.events }
    pub fn max_attempts(&self) -> usize { self.max_attempts }
    pub fn clear_events(&mut self) { self.events.clear(); }

    /// Count events for a tool.
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
        assert_eq!(p.recommend(ErrorClass::RateLimit), RecoveryAction::RetryWithBackoff);
    }

    #[test]
    fn default_auth_is_escalate() {
        let p = RecoveryPolicy::default();
        assert_eq!(p.recommend(ErrorClass::AuthFailure), RecoveryAction::Escalate);
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
        assert_eq!(RecoveryAction::RetryWithBackoff.to_string(), "retry_with_backoff");
        assert_eq!(RecoveryAction::Abort.to_string(), "abort");
    }

    #[test]
    fn max_attempts_getter() {
        let p = RecoveryPolicy::new(5);
        assert_eq!(p.max_attempts(), 5);
    }
}
