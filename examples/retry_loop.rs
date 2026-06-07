//! A runnable example showing how to drive an agent tool call through
//! `agent-error-recovery`.
//!
//! Run with:
//!
//! ```text
//! cargo run --example retry_loop
//! ```

use agent_error_recovery::{ErrorClass, RecoveryAction, RecoveryPolicy};

/// A stand-in for a real tool call. It "fails" with a transient error for the
/// first few attempts, then succeeds.
fn call_tool(attempt: usize) -> Result<&'static str, u16> {
    if attempt < 2 {
        Err(503) // Service Unavailable -> ServerError
    } else {
        Ok("search results")
    }
}

fn main() {
    let mut policy = RecoveryPolicy::new(5);
    let tool = "web_search";

    let mut attempt = 0;
    loop {
        match call_tool(attempt) {
            Ok(result) => {
                println!("attempt {attempt}: success -> {result}");
                break;
            }
            Err(status) => {
                // Classify the failure, defaulting to Unknown if the status is
                // not an error code we recognize.
                let class = ErrorClass::from_status(status).unwrap_or(ErrorClass::Unknown);
                let action = policy.record(tool, class, attempt);
                println!("attempt {attempt}: {class} -> {action}");

                match action {
                    RecoveryAction::RetryImmediate => {}
                    RecoveryAction::RetryWithBackoff => {
                        let delay = RecoveryPolicy::backoff_ms(attempt as u32, 100, 10_000);
                        println!("    backing off for {delay} ms (simulated)");
                    }
                    RecoveryAction::Skip | RecoveryAction::Abort | RecoveryAction::Escalate => {
                        println!("    giving up: {action}");
                        break;
                    }
                }
                attempt += 1;
            }
        }
    }

    println!(
        "\nrecorded {} recovery event(s) for `{tool}`",
        policy.event_count_for(tool)
    );
}
