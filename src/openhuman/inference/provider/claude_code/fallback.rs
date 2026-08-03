//! Startup-failure classification for the model fallback ladder.
//!
//! The projects task runner ([`crate::openhuman::projects::bus`]) uses this to
//! decide, when a claude run errors, whether the failure happened *before*
//! claude started successfully (→ step to the next model on the ladder) or
//! *after* it was running (→ genuine failure, move the task to Blocked).
//!
//! The driver ([`super::driver`]) stamps [`super::driver::STARTUP_FAILURE_PREFIX`]
//! onto errors that occurred before its `system` init event arrived (auth /
//! invalid-model / unreachable-backend / spawn failure). Two other unambiguous
//! startup markers are matched as a safety net: the `from_env` build failure
//! wrap ("failed to build ClaudeCodeProvider:") and the raw spawn error.

use super::driver::STARTUP_FAILURE_PREFIX;

/// Returns true when `err` denotes a failure to *start* claude (as opposed to a
/// failure that happened while it was running). Startup failures are the only
/// ones the fallback ladder steps on.
pub fn is_startup_failure(err: &str) -> bool {
    err.contains(STARTUP_FAILURE_PREFIX.trim())
        // CLI missing / outdated / unusable — wrapped by run_agent (bus.rs).
        || err.contains("failed to build ClaudeCodeProvider:")
        // Raw spawn failure (also carries the prefix, but match defensively).
        || err.contains("failed to spawn `claude`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_prefixed_errors_classify_true() {
        assert!(is_startup_failure(
            "[startup-failure] [claude-code][driver] exit Some(1) stderr=auth error"
        ));
        assert!(is_startup_failure(
            "failed to build ClaudeCodeProvider: [claude-code] `claude` CLI not installed."
        ));
        assert!(is_startup_failure(
            "[startup-failure] failed to spawn `claude`: No such file or directory"
        ));
    }

    #[test]
    fn ran_then_failed_and_timeout_classify_false() {
        // Non-zero exit AFTER the init event (no prefix) = ran but failed.
        assert!(!is_startup_failure(
            "[claude-code][driver] exit Some(1) stderr=task error"
        ));
        // Timeout is a ran-but-stuck failure, not a startup failure.
        assert!(!is_startup_failure(
            "[claude-code][driver] turn timed out after 7200s"
        ));
    }
}
