//! OA-06: address-scoped idempotency and atomic conversion of independently
//! populated 0.3 schema-1 redb files. Fixtures/backups remain for boundary
//! evidence capture. No live store or externally supplied path is accepted.
//!
//! `parking_lot_negative_control="oa-06"` restores global active fences; the
//! `OA-06:address-scope` oracle must then reject the second address's delivery.

#[path = "support/legacy_v03.rs"]
mod legacy_v03;

use std::collections::BTreeMap;
use std::time::Duration;

use legacy_v03::{Fixture, IDENTITY, NOW, address, assignments, old_open, snapshot};
use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LeaseQueue, LegacyDeliveryStatus, LegacyIdempotencyOwner,
    MailboxKey, ResultMailbox, SchedulerError, TaskStatus, TestClock,
};
use prometheus_parking_lot_embedded::EmbeddedStore;

type Snapshot = BTreeMap<String, serde_json::Value>;

fn delivery(key: &MailboxKey, caller: &str) -> Delivery<String> {
    Delivery::new(
        key.clone(),
        99,
        TaskStatus::Completed,
        Some("retry".to_owned()),
    )
    .with_idempotency_key(caller)
}

// This synthetic evidence refers only to the frozen OA-06 fixture, whose
// consumed b/c outcomes were Completed. Owner evidence alone asserts no status.
fn open_migration(
    path: &std::path::Path,
    clock: TestClock,
    owners: &[LegacyIdempotencyOwner],
) -> Result<EmbeddedStore, SchedulerError> {
    let statuses = [
        LegacyDeliveryStatus::new("b-consumed-finite", address("a"), TaskStatus::Completed),
        LegacyDeliveryStatus::new("c-consumed-permanent", address("b"), TaskStatus::Completed),
    ];
    EmbeddedStore::open_with_clock_and_idempotency_metadata(path, clock, owners, &statuses)
}

fn assert_refusal(result: &Result<EmbeddedStore, SchedulerError>) {
    assert!(
        matches!(result, Err(SchedulerError::InvalidConfig { .. })),
        "{result:?}"
    );
}

#[test]
fn legacy_migration_refuses_missing_or_invalid_owners_without_logical_changes() {
    let fixture = Fixture::new(false);
    let before = snapshot(&fixture.path);
    let clock = TestClock::new(u128::from(NOW));
    assert_refusal(&EmbeddedStore::open_with_clock(
        &fixture.path,
        clock.clone(),
    ));
    assert_eq!(
        snapshot(&fixture.path),
        before,
        "ordinary open cannot convert schema 1"
    );

    let known = assignments();
    let mut conflicting = known.clone();
    conflicting.push(LegacyIdempotencyOwner::new("a-retained", address("wrong")));
    let mut duplicate = known.clone();
    duplicate.push(known[0].clone());
    let mut unknown = known.clone();
    unknown.push(LegacyIdempotencyOwner::new("never-existed", address("a")));
    let mut expired = known;
    expired.push(LegacyIdempotencyOwner::new(
        "f-expired-ownerless",
        address("a"),
    ));
    for owners in [Vec::new(), conflicting, duplicate, unknown, expired] {
        assert_refusal(&open_migration(&fixture.path, clock.clone(), &owners));
        assert_eq!(
            snapshot(&fixture.path),
            before,
            "refusal rolls back new tables, partial fences, schema, queue state and identity"
        );
        fixture.assert_backup_untouched();
    }
    // The untouched backup is independently usable by the frozen old protocol.
    let restored = fixture.restore_copy();
    drop(old_open(&restored).expect("restored backup remains schema 1"));
    assert_eq!(snapshot(&restored), before);
    fixture.assert_backup_untouched();
}

#[tokio::test]
async fn legacy_migration_preserves_results_fences_identity_and_original_ttls() {
    let fixture = Fixture::new(false);
    let before = snapshot(&fixture.path);
    let owners = assignments();
    let clock = TestClock::new(u128::from(NOW));
    let converted = assert_legacy_conversion(&fixture, &clock, &owners, &before);
    {
        let store = EmbeddedStore::open_with_clock(&fixture.path, clock.clone())
            .expect("ordinary schema-3 reopen");
        let mailbox = store.result_mailbox::<String>();
        assert_original_results(&mailbox).await;
        assert_original_queue(&store).await;
    }
    assert_original_expiry_and_replay(&fixture, &clock, &owners).await;
    let after = snapshot(&fixture.path);
    assert_eq!(
        after["mb_idem"], before["mb_idem"],
        "all legacy provenance, including expired ownerless, stays immutable"
    );
    assert_eq!(
        after["mb_legacy_owners_v2"],
        converted["mb_legacy_owners_v2"]
    );
    fixture.assert_backup_untouched();
    let restored = fixture.restore_copy();
    drop(old_open(&restored).expect("backup rollback preserves old-reader compatibility"));
    assert_eq!(
        snapshot(&restored),
        before,
        "rollback uses untouched backup, not schema downgrade"
    );
}

fn assert_legacy_conversion(
    fixture: &Fixture,
    clock: &TestClock,
    owners: &[LegacyIdempotencyOwner],
    before: &Snapshot,
) -> Snapshot {
    let store =
        open_migration(&fixture.path, clock.clone(), owners).expect("explicit mapped conversion");
    assert_eq!(store.lease_queue::<String>(10).identity(), IDENTITY);
    drop(store);
    let converted = snapshot(&fixture.path);
    for (table, rows) in before {
        if table != "tables" && table != "q_meta" {
            assert_eq!(
                converted.get(table),
                Some(rows),
                "legacy table {table} unchanged"
            );
        }
    }
    let mut expected_meta = before["q_meta"].clone();
    for row in expected_meta.as_array_mut().expect("metadata rows") {
        if row[0] == "schema_version" {
            row[1] = serde_json::json!(3);
        }
    }
    assert_eq!(
        converted["q_meta"], expected_meta,
        "only layout version changes in queue metadata"
    );
    assert_eq!(
        converted["mb_idem_v2"]
            .as_array()
            .expect("scoped rows")
            .len(),
        5
    );
    assert_eq!(
        converted["mb_legacy_owners_v2"]
            .as_array()
            .expect("owner proofs")
            .len(),
        5
    );

    let old = old_open(&fixture.path);
    assert!(
        old.as_ref()
            .is_err_and(|error| error.contains("layout version 3, this build understands 1")),
        "frozen 0.3 open protocol must reject schema 3"
    );
    drop(old);
    assert_eq!(
        snapshot(&fixture.path),
        converted,
        "refused old open cannot alter store"
    );

    let again = open_migration(&fixture.path, clock.clone(), owners).expect("same mapping replay");
    drop(again);
    assert_eq!(
        snapshot(&fixture.path),
        converted,
        "conversion replay preserves every fence lifetime"
    );
    for bad in [
        vec![LegacyIdempotencyOwner::new(
            "b-consumed-finite",
            address("wrong"),
        )],
        vec![LegacyIdempotencyOwner::new("never-existed", address("a"))],
        vec![owners[0].clone(), owners[0].clone()],
    ] {
        assert_refusal(&open_migration(&fixture.path, clock.clone(), &bad));
        assert_eq!(snapshot(&fixture.path), converted);
    }

    converted
}

async fn assert_original_results(mailbox: &impl ResultMailbox<String>) {
    let a = address("a");
    let b = address("b");
    for (key, caller) in [
        (&a, "a-retained"),
        (&a, "b-consumed-finite"),
        (&b, "c-consumed-permanent"),
        (&a, "d-reused"),
        (&a, "e-live-finite"),
    ] {
        assert_eq!(
            mailbox.deliver(delivery(key, caller)).await.expect("retry"),
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
    }
    // Expired earlier-generation row belonged to b; only the live exact
    // expiry generation at a owns d-reused's migrated fence.
    assert_eq!(
        mailbox
            .deliver(delivery(&b, "d-reused"))
            .await
            .expect("other generation owner"),
        DeliveryOutcome::Stored
    );
    assert_eq!(
        mailbox
            .deliver(delivery(&b, "b-consumed-finite"))
            .await
            .expect("other address"),
        DeliveryOutcome::Stored
    );
    let mut received = Vec::new();
    while let Some(entry) = mailbox.consume(&a).await.expect("legacy consume") {
        received.push((
            entry.task_id,
            entry.payload.expect("payload"),
            entry.delivered_at_ms,
        ));
    }
    assert_eq!(
        received,
        vec![
            (1, "legacy first".to_owned(), u128::from(NOW - 100)),
            (2, "unkeyed".to_owned(), u128::from(NOW - 100)),
            (4, "new generation".to_owned(), u128::from(NOW)),
            (5, "finite live".to_owned(), u128::from(NOW - 100)),
        ],
        "original bytes decode with original order/timestamps, never a synthesized result"
    );
    assert_eq!(
        mailbox
            .deliver(delivery(&a, "a-retained"))
            .await
            .expect("consumed retry"),
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
}

async fn assert_original_queue(store: &EmbeddedStore) {
    let queue = store.lease_queue::<String>(10);
    assert_eq!(queue.identity(), IDENTITY);
    let depth = queue.depth().await.expect("preserved queue");
    assert_eq!(
        (
            depth.ready,
            depth.delayed,
            depth.leased,
            depth.dead_lettered
        ),
        (1, 1, 1, 1)
    );
    let leased = queue
        .lease(1, Duration::from_secs(30))
        .await
        .expect("original queued task");
    assert_eq!(leased.len(), 1);
    assert_eq!(leased[0].task.payload, "ready");
}

async fn assert_original_expiry_and_replay(
    fixture: &Fixture,
    clock: &TestClock,
    owners: &[LegacyIdempotencyOwner],
) {
    clock.advance(Duration::from_millis(1_000));
    {
        let store = EmbeddedStore::open_with_clock(&fixture.path, clock.clone())
            .expect("reopen at original expiry");
        let mailbox = store.result_mailbox::<String>();
        mailbox.evict_expired().await.expect("evict elapsed fences");
    }
    let evicted = snapshot(&fixture.path);
    {
        // Owner proof must remain usable after the active scoped fence has
        // been removed. Replay must not reconstruct that fence from mb_idem.
        let store = open_migration(&fixture.path, clock.clone(), owners)
            .expect("mapping replay after fence eviction");
        drop(store);
        assert_eq!(
            snapshot(&fixture.path),
            evicted,
            "mapping replay never resurrects an evicted fence"
        );
        let store = EmbeddedStore::open_with_clock(&fixture.path, clock.clone())
            .expect("open after replay");
        let mailbox = store.result_mailbox::<String>();
        assert_eq!(
            mailbox
                .deliver(delivery(&address("a"), "b-consumed-finite"))
                .await
                .expect("original expiry"),
            DeliveryOutcome::Stored
        );
        assert_eq!(
            mailbox
                .deliver(delivery(&address("b"), "b-consumed-finite"))
                .await
                .expect("independent lifetime"),
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
        assert_eq!(
            mailbox
                .deliver(delivery(&address("b"), "c-consumed-permanent"))
                .await
                .expect("permanent tombstone"),
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
        assert_eq!(
            mailbox
                .deliver(delivery(&address("a"), "a-retained"))
                .await
                .expect("consumed permanent"),
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
    }
}

#[test]
fn legacy_migration_requires_authority_for_ambiguous_exact_generations() {
    let fixture = Fixture::new(true);
    let before = snapshot(&fixture.path);
    let clock = TestClock::new(u128::from(NOW));
    let mut owners = assignments();
    assert_refusal(&open_migration(&fixture.path, clock.clone(), &owners));
    assert_eq!(snapshot(&fixture.path), before);
    owners.push(LegacyIdempotencyOwner::new(
        "g-ambiguous",
        address("outside-candidates"),
    ));
    assert_refusal(&open_migration(&fixture.path, clock.clone(), &owners));
    assert_eq!(snapshot(&fixture.path), before);
    owners.pop();
    owners.push(LegacyIdempotencyOwner::new("g-ambiguous", address("a")));
    let store = open_migration(&fixture.path, clock, &owners)
        .expect("authoritative candidate resolves ambiguity");
    drop(store);
    let after = snapshot(&fixture.path);
    assert_eq!(after["mb_entries"], before["mb_entries"]);
    assert_eq!(after["mb_idem"], before["mb_idem"]);
    fixture.assert_backup_untouched();
}

#[tokio::test]
async fn mailbox_scope_distinguishes_optional_and_delimited_addresses() {
    let dir = tempfile::tempdir().expect("fresh isolated store");
    let path = dir.path().join("scope.redb");
    let clock = TestClock::new(u128::from(NOW));
    let addresses = scoped_addresses();
    {
        let store = EmbeddedStore::open_with_clock(&path, clock.clone()).expect("fresh schema 3");
        let mailbox = store.result_mailbox::<String>();
        for (index, key) in addresses.iter().enumerate() {
            let first = Delivery::new(
                key.clone(),
                1,
                TaskStatus::Completed,
                Some(format!("first-{index}")),
            )
            .with_idempotency_key("shared|caller\u{1f}key");
            assert_eq!(
                mailbox.deliver(first).await.expect("first"),
                DeliveryOutcome::Stored,
                "OA-06:address-scope: another complete address suppressed {key:?}"
            );
            assert_eq!(
                mailbox
                    .deliver(delivery(key, "shared|caller\u{1f}key"))
                    .await
                    .expect("retry"),
                DeliveryOutcome::Duplicate {
                    status: TaskStatus::Completed
                }
            );
            assert_eq!(
                mailbox
                    .consume(key)
                    .await
                    .expect("consume")
                    .expect("first result")
                    .payload,
                Some(format!("first-{index}"))
            );
        }
        // Address/caller concatenation would collide if scope structure were
        // flattened; both pairs must retain independent caller keys as well.
        let left = MailboxKey {
            session_id: Some("session|caller".to_owned()),
            ..address("tenant")
        };
        let right = address("tenant");
        assert_eq!(
            mailbox
                .deliver(delivery(&left, "key"))
                .await
                .expect("left pair"),
            DeliveryOutcome::Stored
        );
        assert_eq!(
            mailbox
                .deliver(delivery(&right, "caller|key"))
                .await
                .expect("right pair"),
            DeliveryOutcome::Stored
        );
    }
    let store = EmbeddedStore::open_with_clock(&path, clock).expect("restart scoped fences");
    let mailbox = store.result_mailbox::<String>();
    for key in &addresses {
        assert_eq!(
            mailbox
                .deliver(delivery(key, "shared|caller\u{1f}key"))
                .await
                .expect("restart retry"),
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
        assert!(
            mailbox
                .consume(key)
                .await
                .expect("no second result")
                .is_none()
        );
    }
}

fn scoped_addresses() -> [MailboxKey; 8] {
    let base = MailboxKey {
        tenant: "t".to_owned(),
        user_id: None,
        session_id: None,
    };
    [
        base.clone(),
        address("other-tenant"),
        MailboxKey {
            user_id: Some(String::new()),
            ..base.clone()
        },
        MailboxKey {
            session_id: Some(String::new()),
            ..base.clone()
        },
        MailboxKey {
            user_id: Some("u|s".to_owned()),
            session_id: Some("tail".to_owned()),
            ..base.clone()
        },
        MailboxKey {
            user_id: Some("u".to_owned()),
            session_id: Some("s|tail".to_owned()),
            ..base
        },
        MailboxKey {
            tenant: "t\u{1f}u".to_owned(),
            user_id: None,
            session_id: Some("s".to_owned()),
        },
        MailboxKey {
            tenant: "t".to_owned(),
            user_id: Some("u".to_owned()),
            session_id: Some("\u{1f}s".to_owned()),
        },
    ]
}
