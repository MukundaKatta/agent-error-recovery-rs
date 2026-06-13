# agent-error-recovery

Composable error recovery strategies for LLM agent tool calls.

When an LLM agent invokes tools, those calls fail in predictable ways: rate
limits, network timeouts, auth failures, invalid input, and so on. This crate
provides a small, dependency-free policy engine that classifies such errors and
recommends a recovery action (retry, back off, abort, skip, or escalate), with
per-error-class overrides and a built-in attempt log.

## Features

- **Error classification** via the `ErrorClass` enum (`RateLimit`,
  `NetworkTimeout`, `AuthFailure`, `InvalidInput`, `NotFound`, `ServerError`,
  `Unknown`).
- **Recovery actions** via the `RecoveryAction` enum (`RetryImmediate`,
  `RetryWithBackoff`, `Abort`, `Skip`, `Escalate`).
- **Configurable policy** — sensible defaults per error class, overridable with
  `RecoveryPolicy::set`.
- **Attempt tracking** — `record` automatically escalates once `max_attempts`
  is reached and logs every recovery event.
- **Zero dependencies** — pure `std`.

## Default policy

| Error class      | Default action       |
| ---------------- | -------------------- |
| `RateLimit`      | `RetryWithBackoff`   |
| `NetworkTimeout` | `RetryWithBackoff`   |
| `ServerError`    | `RetryWithBackoff`   |
| `AuthFailure`    | `Escalate`           |
| `InvalidInput`   | `Abort`              |
| `NotFound`       | `Skip`               |
| `Unknown`        | `Abort`              |

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
agent-error-recovery = { git = "https://github.com/MukundaKatta/agent-error-recovery-rs" }
```

## Usage

```rust
use agent_error_recovery::{RecoveryPolicy, RecoveryAction, ErrorClass};

let policy = RecoveryPolicy::default();
let action = policy.recommend(ErrorClass::RateLimit);
assert_eq!(action, RecoveryAction::RetryWithBackoff);
```

### Overriding a default

```rust
use agent_error_recovery::{RecoveryPolicy, RecoveryAction, ErrorClass};

let mut policy = RecoveryPolicy::default();
policy.set(ErrorClass::InvalidInput, RecoveryAction::Skip);
assert_eq!(policy.recommend(ErrorClass::InvalidInput), RecoveryAction::Skip);
```

### Tracking attempts and escalating

`record` returns the recommended action for a given attempt. Once the attempt
count reaches `max_attempts`, it escalates instead of retrying, and logs the
event.

```rust
use agent_error_recovery::{RecoveryPolicy, RecoveryAction, ErrorClass};

let mut policy = RecoveryPolicy::new(3);

// Under the limit -> follow the policy.
assert_eq!(
    policy.record("search_tool", ErrorClass::RateLimit, 1),
    RecoveryAction::RetryWithBackoff,
);

// At the limit -> escalate.
assert_eq!(
    policy.record("search_tool", ErrorClass::RateLimit, 3),
    RecoveryAction::Escalate,
);

assert_eq!(policy.event_count_for("search_tool"), 2);
```

## API overview

- `RecoveryPolicy::default()` — policy with the default mappings above and
  `max_attempts = 3`.
- `RecoveryPolicy::new(max_attempts)` — same defaults with a custom attempt
  limit.
- `set(class, action)` — override the action for an error class.
- `recommend(class)` — the recommended action for an error class.
- `record(tool, class, attempt)` — log an event and return the action,
  escalating at `max_attempts`.
- `events()` / `event_count_for(tool)` / `clear_events()` — inspect and reset
  the event log.
- `max_attempts()` — the configured attempt limit.

## Building and testing

```sh
cargo build
cargo test
```

## Tech stack

- Language: Rust (edition 2021)
- Dependencies: none (standard library only)

## License

Licensed under the MIT License.
