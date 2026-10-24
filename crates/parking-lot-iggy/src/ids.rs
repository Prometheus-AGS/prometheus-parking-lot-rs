//! Message ids.
//!
//! [`message_id`] is the `UUIDv5` of a scope and a stable key under a fixed
//! namespace, as a `u128` (Iggy's message id type). The same scope and key give
//! the same id in every process and after every restart, so a retried publish
//! can be recognized and skipped. It is used where a stable key exists: the
//! idempotency key of a `mailbox.ready` notification, and a stream key with a
//! chunk's sequence number.
//!
//! Task ids are caller-supplied and can repeat, so no id is derived from one
//! alone. A `mailbox.ready` notification for a result without an idempotency
//! key, and every lifecycle event, get an id from a per-instance nonce and a
//! sequence number instead: unique, but not reproducible across processes.
//! Lifecycle events also bypass the publisher's repeat-skipping window.

#[cfg(not(parking_lot_negative_control = "s-11"))]
use uuid::Uuid;

/// Namespace of every id this crate derives.
#[cfg(not(parking_lot_negative_control = "s-11"))]
const NAMESPACE_NAME: &[u8] = b"urn:prometheus-parking-lot:iggy-events:v1";

/// Separator between scope and key (the ASCII unit separator, which does not
/// occur in the scopes below).
#[cfg(not(parking_lot_negative_control = "s-11"))]
const SEP: char = '\u{1f}';

/// The id for `key` within `scope` (for example `mailbox.ready`).
///
/// # Example
///
/// ```
/// use prometheus_parking_lot_iggy::ids::message_id;
///
/// assert_eq!(message_id("mailbox.ready", "job-1"), message_id("mailbox.ready", "job-1"));
/// assert_ne!(message_id("mailbox.ready", "job-1"), message_id("mailbox.ready", "job-2"));
/// ```
#[must_use]
#[cfg(not(parking_lot_negative_control = "s-11"))]
pub fn message_id(scope: &str, key: &str) -> u128 {
    let namespace = Uuid::new_v5(&Uuid::NAMESPACE_URL, NAMESPACE_NAME);
    Uuid::new_v5(&namespace, format!("{scope}{SEP}{key}").as_bytes()).as_u128()
}

/// Negative control for scenario S-11: a random id on every call, so a retried
/// publish never carries the same id twice.
#[must_use]
#[cfg(parking_lot_negative_control = "s-11")]
pub fn message_id(scope: &str, key: &str) -> u128 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};

    let state = RandomState::new();
    let mut high = state.build_hasher();
    (scope, key, 0_u8).hash(&mut high);
    let mut low = state.build_hasher();
    (scope, key, 1_u8).hash(&mut low);
    (u128::from(high.finish()) << 64) | u128::from(low.finish())
}

/// A value that differs between publisher instances, including across
/// restarts, for ids of messages that have no stable key of their own.
pub(crate) fn instance_nonce() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    nanos
        ^ (u128::from(std::process::id()) << 96)
        ^ (u128::from(COUNTER.fetch_add(1, Ordering::Relaxed)) << 64)
}

/// The message id of a `mailbox.ready` notification: one per stored result.
#[must_use]
pub fn mailbox_ready_id(stable_key: &str) -> u128 {
    message_id("mailbox.ready", stable_key)
}

#[cfg(all(test, not(parking_lot_negative_control = "s-11")))]
mod tests {
    use super::*;

    #[test]
    fn ids_are_deterministic_and_scoped() {
        assert_eq!(mailbox_ready_id("k"), mailbox_ready_id("k"));
        assert_ne!(mailbox_ready_id("k"), mailbox_ready_id("l"));
        assert_ne!(mailbox_ready_id("k"), message_id("lifecycle", "k"));
    }

    #[test]
    fn id_is_a_version_5_uuid() {
        let id = Uuid::from_u128(mailbox_ready_id("k"));
        assert_eq!(id.get_version_num(), 5);
    }

    #[test]
    fn known_vector_is_stable_across_releases() {
        // Pinned so a change to the namespace or the scope encoding (which
        // would break de-duplication against already-published messages) fails.
        assert_eq!(
            Uuid::from_u128(mailbox_ready_id("job-1")).to_string(),
            "01db69cb-530f-5dd3-8a15-8be11e711ff0"
        );
    }
}
