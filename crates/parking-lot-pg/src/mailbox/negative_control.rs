//! Test hook of the `s-7` negative control (scenario S-7); compiled only under
//! `--cfg parking_lot_negative_control="s-7"`.
//!
//! The controlled `consume` selects, then deletes in a separate statement.
//! To make the double delivery deterministic instead of a matter of timing,
//! two armed consumers meet on a barrier between the two statements: each has
//! read the entry before either deletes it.
//!
//! Arming is process-wide, so run the control with a runner that isolates
//! tests in processes (`cargo nextest`).

use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Barrier;

static ARMED: AtomicBool = AtomicBool::new(false);
static GAP: LazyLock<Barrier> = LazyLock::new(|| Barrier::new(2));

/// From now on every consume waits, between its read and its delete, for one
/// other consume to reach the same point.
pub fn arm_consume_gap() {
    ARMED.store(true, Ordering::SeqCst);
}

pub(super) async fn gap() {
    if ARMED.load(Ordering::SeqCst) {
        GAP.wait().await;
    }
}
