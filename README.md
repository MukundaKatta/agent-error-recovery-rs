# agent-error-recovery

[![CI](https://github.com/MukundaKatta/agent-error-recovery-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MukundaKatta/agent-error-recovery-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

Composable error-recovery strategies for LLM agent tool calls.

When an LLM agent invokes a tool (an HTTP API, a database query, a shell
command, ...), that call can fail in many ways: rate limits, timeouts, auth
errors, bad input, missing resources, server faults. This small, dependency-free
crate gives you a single place to **classify** those failures and **decide what
to do next** — retry, back off, skip, abort, or escalate — together with a
running **log** of the decisions made so you can inspect or report on them.

It is intentionally a *policy* library: it does not perform I/O, sleep, or make
network calls. You stay in control of how recovery actions are carried out.

## Features

- `ErrorClass` — a compact taxonomy of failure categories, with a best-effort
  `from_status` HTTP-status classifier and an `is_transient` helper.
- `RecoveryAction` — the set of recommended responses, with an `is_retry` helper.
- `RecoveryPolicy` — maps error classes to actions, supports per-class
  overrides, enforces a maximum attempt count (escalating instead of looping
  forever), and records an event log.
- `RecoveryPolicy::backoff_ms` — a pure, overflow-safe exponential-backoff
  delay calculator.
- Zero dependencies, `#![no_std]`-friendly in spirit (uses only `std`
  collections today), and fully unit- and integration-tested.

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
agent-error-recovery = { git = "https://github.com/MukundaKatta/agent-error-recovery-rs" }
```

(Once published to crates.io you can instead use `agent-error-recovery = "0.1"`.)

## Usage

Classify a failed tool call, ask the policy what to do, and act on the
recommendation:

```rust
use agent_error_recovery::{ErrorClass, RecoveryAction, RecoveryPolicy};

// Allow up to 5 attempts before escalating.
let mut policy = RecoveryPolicy::new(5);

// A tool call returned HTTP 503.
let class = ErrorClass::from_status(503).unwrap_or(ErrorClass::Unknown);
assert_eq!(class, ErrorClass::ServerError);

// `record` logs the event and returns the action to take for this attempt.
let attempt = 0;
let action = policy.record("web_search", class, attempt);
assert_eq!(action, RecoveryAction::RetryWithBackoff);

if action.is_retry() {
    // Compute a backoff delay you can pass to your own timer. This helper
    // never sleeps and never overflows.
    let delay_ms = RecoveryPolicy::backoff_ms(attempt as u32, 100, 10_000);
    assert_eq!(delay_ms, 100);
}
```

Override the action for a specific error class:

```rust
use agent_error_recovery::{ErrorClass, RecoveryAction, RecoveryPolicy};

let mut policy = RecoveryPolicy::default();
// By default a 404 is skipped; treat it as fatal instead.
policy.set(ErrorClass::NotFound, RecoveryAction::Abort);
assert_eq!(policy.recommend(ErrorClass::NotFound), RecoveryAction::Abort);
```

A complete, runnable retry loop lives in
[`examples/retry_loop.rs`](examples/retry_loop.rs):

```text
cargo run --example retry_loop
```

## Default policy

| Error class       | Default action        | Rationale                              |
| ----------------- | --------------------- | -------------------------------------- |
| `RateLimit`       | `RetryWithBackoff`    | Transient; back off and try again      |
| `NetworkTimeout`  | `RetryWithBackoff`    | Transient; back off and try again      |
| `ServerError`     | `RetryWithBackoff`    | Often transient (5xx)                  |
| `AuthFailure`     | `Escalate`            | Needs new credentials / human          |
| `InvalidInput`    | `Abort`               | Retrying the same input won't help     |
| `NotFound`        | `Skip`                | The resource is gone; move on          |
| `Unknown`         | `Abort`               | Fail safe                              |

Regardless of the class mapping, once `attempt` reaches the policy's
`max_attempts`, `record` returns `RecoveryAction::Escalate` so retries can never
loop forever.

## API overview

### `enum ErrorClass`

`RateLimit`, `NetworkTimeout`, `AuthFailure`, `InvalidInput`, `NotFound`,
`ServerError`, `Unknown`.

- `ErrorClass::ALL` — array of every variant, for iteration.
- `ErrorClass::from_status(status: u16) -> Option<ErrorClass>` — classify an
  HTTP status code (returns `None` for non-error codes < 400).
- `ErrorClass::is_transient(&self) -> bool` — whether the class is typically
  worth retrying.

### `enum RecoveryAction`

`RetryImmediate`, `RetryWithBackoff`, `Abort`, `Skip`, `Escalate`.

- `RecoveryAction::is_retry(&self) -> bool` — whether the action asks the caller
  to retry.

### `struct RecoveryPolicy`

- `RecoveryPolicy::default()` — sensible defaults, `max_attempts = 3`.
- `RecoveryPolicy::new(max_attempts: usize)` — defaults with a custom cap.
- `set(&mut self, class, action)` — override the action for a class.
- `recommend(&self, class) -> RecoveryAction` — the action for a class
  (falls back to `Abort` for unmapped classes).
- `record(&mut self, tool, class, attempt) -> RecoveryAction` — log an event and
  return the action, escalating once `attempt >= max_attempts`.
- `RecoveryPolicy::backoff_ms(attempt, base_ms, max_ms) -> u64` — exponential
  backoff (`base_ms * 2^attempt`, clamped to `max_ms`, overflow-safe).
- `events()`, `event_count_for(tool)`, `clear_events()`, `max_attempts()`.

### `struct RecoveryEvent`

A logged decision: `tool_name`, `error_class`, `action`, `attempt`.

## Development

```sh
cargo build
cargo test            # unit + integration + doc tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo run --example retry_loop
```

## License

Licensed under the [MIT License](https://opensource.org/licenses/MIT).
