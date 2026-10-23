//! Crash-point hooks for the crash-safety tests.
//!
//! With the `fault-injection` feature, a process whose environment sets
//! `PARKING_LOT_FAULT_POINT=<operation>:<phase>` (for example
//! `ack:before_commit`) stops dead the first time that point is reached: it
//! reports the point on stdout and parks forever, holding the open write
//! transaction, until the parent test sends it SIGKILL. Without the feature
//! every hook compiles to nothing.

/// Where in an operation's write transaction a crash point sits.
#[derive(Debug, Clone, Copy)]
pub enum Phase {
    /// All writes are applied to the open transaction; nothing is committed.
    BeforeCommit,
    /// The transaction is durably committed; the caller has not been told.
    AfterCommit,
}

#[cfg(feature = "fault-injection")]
pub fn point(operation: &str, phase: Phase) {
    use std::io::Write;
    use std::sync::OnceLock;

    static ARMED: OnceLock<Option<String>> = OnceLock::new();
    let phase = match phase {
        Phase::BeforeCommit => "before_commit",
        Phase::AfterCommit => "after_commit",
    };
    let armed = ARMED.get_or_init(|| std::env::var("PARKING_LOT_FAULT_POINT").ok());
    let hit = armed
        .as_deref()
        .and_then(|point| point.split_once(':'))
        .is_some_and(|(op, ph)| op == operation && ph == phase);
    if hit {
        println!("CRASHPOINT {operation}:{phase}");
        let _ = std::io::stdout().flush();
        loop {
            std::thread::park();
        }
    }
}

#[cfg(not(feature = "fault-injection"))]
#[inline(always)]
pub const fn point(_operation: &str, _phase: Phase) {}
