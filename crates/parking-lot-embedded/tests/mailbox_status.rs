//! OA-07 real-file status durability and explicit schema-1/schema-2 conversion.
//! Raw legacy fixtures and untouched backups are retained for final SHA256
//! binding. `oa-07` restores retry-derived duplicate status and must fail the
//! named stored-status/consumed-status oracles, with real redb still in use.

#[path = "support/legacy_metadata.rs"]
mod legacy_metadata;
#[path = "support/legacy_v03.rs"]
#[allow(
    dead_code,
    reason = "frozen fixture support also serves the OA-06 integration binary"
)]
mod legacy_v03;

use std::time::Duration;

use legacy_metadata::{
    MetadataFixture, finite_failure, old_v2_open, permanent_failure, retained_failure, snapshot,
    statuses,
};
use legacy_v03::{IDENTITY, NOW, address, assignments, old_open};
use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LeaseQueue, LegacyDeliveryStatus, LegacyIdempotencyOwner,
    ResultMailbox, SchedulerError, TaskError, TaskStatus, TestClock,
};
use prometheus_parking_lot_embedded::EmbeddedStore;
use redb::{Database, TableDefinition};

fn retry(key: prometheus_parking_lot_core::MailboxKey, caller: &str) -> Delivery<String> {
    Delivery::new(
        key,
        99,
        TaskStatus::Completed,
        Some("successful retry".to_owned()),
    )
    .with_idempotency_key(caller)
}

fn refused(result: Result<EmbeddedStore, SchedulerError>) {
    assert!(
        matches!(result, Err(SchedulerError::InvalidConfig { .. })),
        "{result:?}"
    );
}

#[test]
fn legacy_status_conversion_refuses_unknown_or_contradictory_metadata_atomically() {
    for schema in [1, 2] {
        let fixture = MetadataFixture::new(schema, false);
        let before = snapshot(&fixture.path);
        let clock = TestClock::new(u128::from(NOW));
        let owners = if schema == 1 {
            assignments()
        } else {
            Vec::new()
        };
        refused(EmbeddedStore::open_with_clock(&fixture.path, clock.clone()));
        refused(EmbeddedStore::open_with_clock_and_idempotency_owners(
            &fixture.path,
            clock.clone(),
            &owners,
        ));
        assert_eq!(
            snapshot(&fixture.path),
            before,
            "owner-only migration must not commit an intermediate schema before status refusal"
        );
        let known = statuses(schema);
        if schema == 1 {
            refused(EmbeddedStore::open_with_clock_and_idempotency_metadata(
                &fixture.path,
                clock.clone(),
                &[],
                &known,
            ));
            assert_eq!(
                snapshot(&fixture.path),
                before,
                "status evidence does not assert ownership"
            );
        }
        let mut unknown = known.clone();
        unknown.push(LegacyDeliveryStatus::new(
            "absent",
            address("a"),
            TaskStatus::Completed,
        ));
        let mut duplicate = known.clone();
        duplicate.push(known[0].clone());
        let mut contradictory = known.clone();
        contradictory.push(LegacyDeliveryStatus::new(
            "a-retained",
            address("a"),
            TaskStatus::Completed,
        ));
        let mut expired = known.clone();
        expired.push(LegacyDeliveryStatus::new(
            if schema == 1 {
                "f-expired-ownerless"
            } else {
                "v2-expired"
            },
            address(if schema == 1 { "a" } else { "b" }),
            TaskStatus::Completed,
        ));
        let mut wrong_owner = known.clone();
        wrong_owner[0] =
            LegacyDeliveryStatus::new("b-consumed-finite", address("b"), finite_failure());
        for evidence in [
            Vec::new(),
            vec![known[0].clone()],
            unknown,
            duplicate,
            contradictory,
            expired,
            wrong_owner,
        ] {
            refused(EmbeddedStore::open_with_clock_and_idempotency_metadata(
                &fixture.path,
                clock.clone(),
                &owners,
                &evidence,
            ));
            assert_eq!(
                snapshot(&fixture.path),
                before,
                "refusal rolls back schema, partial status/owner writes, raw rows, TTL and identity"
            );
            fixture.assert_backup_untouched();
        }
    }
}

#[tokio::test]
async fn legacy_metadata_conversion_preserves_original_status_through_consume_restart_and_ttl() {
    for schema in [1, 2] {
        let fixture = MetadataFixture::new(schema, false);
        let before = snapshot(&fixture.path);
        let clock = TestClock::new(u128::from(NOW));
        let owners = if schema == 1 {
            assignments()
        } else {
            Vec::new()
        };
        let evidence = statuses(schema);
        let store = EmbeddedStore::open_with_clock_and_idempotency_metadata(
            &fixture.path,
            clock.clone(),
            &owners,
            &evidence,
        )
        .expect("authoritative metadata conversion");
        assert_eq!(store.lease_queue::<String>(10).identity(), IDENTITY);
        drop(store);
        let converted = snapshot(&fixture.path);
        for (table, rows) in &before {
            if table != "tables" && table != "q_meta" {
                assert_eq!(
                    converted.get(table),
                    Some(rows),
                    "raw source table {table} remains unchanged"
                );
            }
        }
        let mut expected_meta = before["q_meta"].clone();
        for row in expected_meta.as_array_mut().expect("queue metadata rows") {
            if row[0] == "schema_version" {
                row[1] = serde_json::json!(3);
            }
        }
        assert_eq!(converted["q_meta"], expected_meta);
        assert!(
            old_open(&fixture.path).is_err(),
            "schema-1 writer rejects new layout"
        );
        assert!(
            old_v2_open(&fixture.path).is_err(),
            "schema-2 writer rejects new layout"
        );
        assert_eq!(
            snapshot(&fixture.path),
            converted,
            "old writer refusal leaves all logical rows unchanged"
        );
        {
            let store = EmbeddedStore::open_with_clock(&fixture.path, clock.clone())
                .expect("schema-3 open");
            let mailbox = store.result_mailbox::<String>();
            assert_eq!(
                mailbox
                    .deliver(retry(address("a"), "a-retained"))
                    .await
                    .expect("retained failure retry"),
                DeliveryOutcome::Duplicate {
                    status: retained_failure()
                },
                "OA-07:stored-status: migration used retry status"
            );
            let peeked = mailbox
                .peek(&address("a"))
                .await
                .expect("non-destructive original peek")
                .expect("original result");
            assert_eq!(peeked.status, retained_failure());
            assert_eq!(peeked.payload.as_deref(), Some("legacy first"));
            assert_eq!(
                mailbox
                    .deliver(retry(address("a"), "b-consumed-finite"))
                    .await
                    .expect("consumed finite retry"),
                DeliveryOutcome::Duplicate {
                    status: finite_failure()
                },
                "OA-07:consumed-status: consumed finite failure lost"
            );
            assert_eq!(
                mailbox
                    .deliver(retry(address("b"), "c-consumed-permanent"))
                    .await
                    .expect("consumed permanent retry"),
                DeliveryOutcome::Duplicate {
                    status: permanent_failure()
                }
            );
            assert_eq!(
                mailbox
                    .deliver(retry(address("a"), "d-reused"))
                    .await
                    .expect("exact reused generation"),
                DeliveryOutcome::Duplicate {
                    status: TaskStatus::Running
                },
                "expired earlier Failed is not current Running proof"
            );
            assert_eq!(
                mailbox
                    .deliver(retry(address("a"), "e-live-finite"))
                    .await
                    .expect("generic queued status"),
                DeliveryOutcome::Duplicate {
                    status: TaskStatus::Queued
                }
            );
            if schema == 2 {
                for item in evidence
                    .iter()
                    .filter(|item| item.idempotency_key == "z-consumed")
                {
                    assert_eq!(
                        mailbox
                            .deliver(retry(item.key.clone(), "z-consumed"))
                            .await
                            .expect("independent scoped status"),
                        DeliveryOutcome::Duplicate {
                            status: item.status.clone()
                        }
                    );
                }
                assert_eq!(
                    mailbox
                        .deliver(retry(address("a"), "same-scoped"))
                        .await
                        .expect("scoped expired status"),
                    DeliveryOutcome::Duplicate {
                        status: TaskStatus::Expired
                    }
                );
                assert_eq!(
                    mailbox
                        .deliver(retry(address("b"), "same-scoped"))
                        .await
                        .expect("scoped drop status"),
                    DeliveryOutcome::Duplicate {
                        status: TaskStatus::Dropped("original drop".to_owned())
                    }
                );
            }
            let mut consumed = Vec::new();
            while let Some(entry) = mailbox
                .consume(&address("a"))
                .await
                .expect("client consumes originals")
            {
                consumed.push(entry.task_id);
            }
            assert_eq!(
                consumed,
                if schema == 1 {
                    vec![1, 2, 4, 5]
                } else {
                    vec![1, 2, 4, 5, 6]
                },
                "each original result remains consumable exactly once, in original order"
            );
        }
        let consumed = snapshot(&fixture.path);
        {
            let store = EmbeddedStore::open_with_clock_and_idempotency_metadata(
                &fixture.path,
                clock.clone(),
                &owners,
                &evidence,
            )
            .expect("metadata replay after consume");
            let mailbox = store.result_mailbox::<String>();
            assert_eq!(
                mailbox
                    .deliver(retry(address("a"), "a-retained"))
                    .await
                    .expect("restart after consume"),
                DeliveryOutcome::Duplicate {
                    status: retained_failure()
                },
                "OA-07:consumed-status: restart used retry status"
            );
            assert!(
                mailbox
                    .peek(&address("a"))
                    .await
                    .expect("no replacement")
                    .is_none()
            );
        }
        assert_eq!(
            snapshot(&fixture.path),
            consumed,
            "consume/retry cannot erase immutable status or recreate results"
        );
        let wrong = [LegacyDeliveryStatus::new(
            "b-consumed-finite",
            address("a"),
            TaskStatus::Completed,
        )];
        refused(EmbeddedStore::open_with_clock_and_idempotency_metadata(
            &fixture.path,
            clock.clone(),
            &owners,
            &wrong,
        ));
        assert_eq!(snapshot(&fixture.path), consumed);
        clock.advance(Duration::from_millis(1_000));
        {
            let store = EmbeddedStore::open_with_clock(&fixture.path, clock.clone())
                .expect("open at original expiry");
            store
                .result_mailbox::<String>()
                .evict_expired()
                .await
                .expect("original finite fence expires");
        }
        let evicted = snapshot(&fixture.path);
        let replay = EmbeddedStore::open_with_clock_and_idempotency_metadata(
            &fixture.path,
            clock.clone(),
            &owners,
            &evidence,
        )
        .expect("evidence replay after active fence eviction");
        drop(replay);
        assert_eq!(
            snapshot(&fixture.path),
            evicted,
            "metadata replay never resurrects or extends expired fence"
        );
        {
            let store = EmbeddedStore::open_with_clock(&fixture.path, clock.clone())
                .expect("new generation open");
            let mailbox = store.result_mailbox::<String>();
            assert_eq!(
                mailbox
                    .deliver(retry(address("a"), "b-consumed-finite"))
                    .await
                    .expect("new status generation"),
                DeliveryOutcome::Stored
            );
            let changed_retry = Delivery::new(address("a"), 100, finite_failure(), None::<String>)
                .with_idempotency_key("b-consumed-finite");
            assert_eq!(
                mailbox
                    .deliver(changed_retry)
                    .await
                    .expect("new first-write status"),
                DeliveryOutcome::Duplicate {
                    status: TaskStatus::Completed
                }
            );
            assert_eq!(
                mailbox
                    .deliver(retry(address("b"), "c-consumed-permanent"))
                    .await
                    .expect("permanent consumed fence"),
                DeliveryOutcome::Duplicate {
                    status: permanent_failure()
                }
            );
        }
        let new_generation = snapshot(&fixture.path);
        drop(
            EmbeddedStore::open_with_clock_and_idempotency_metadata(
                &fixture.path,
                clock.clone(),
                &owners,
                &evidence,
            )
            .expect("original proof may validate without replacing new generation"),
        );
        assert_eq!(snapshot(&fixture.path), new_generation);
        assert_eq!(
            new_generation["mb_legacy_metadata_v3"],
            converted["mb_legacy_metadata_v3"]
        );
        fixture.assert_backup_untouched();
        let restored = fixture.restore_copy();
        if schema == 1 {
            drop(old_open(&restored).expect("restore original schema1 protocol"));
        } else {
            drop(old_v2_open(&restored).expect("restore original schema2 protocol"));
        }
        assert_eq!(
            snapshot(&restored),
            before,
            "rollback restores backup, never downgrades converted data"
        );
    }
}

#[test]
fn legacy_metadata_cannot_mix_owner_and_status_from_different_retained_rows() {
    let fixture = MetadataFixture::new(1, true);
    let before = snapshot(&fixture.path);
    let clock = TestClock::new(u128::from(NOW));
    let mut owners = assignments();
    owners.push(LegacyIdempotencyOwner::new("a-retained", address("a")));
    let mut evidence = statuses(1);
    evidence.push(LegacyDeliveryStatus::new(
        "a-retained",
        address("a"),
        TaskStatus::Completed,
    ));
    refused(EmbeddedStore::open_with_clock_and_idempotency_metadata(
        &fixture.path,
        clock.clone(),
        &owners,
        &evidence,
    ));
    assert_eq!(
        snapshot(&fixture.path),
        before,
        "Completed belongs to b; owner a proves Failed"
    );
    evidence.pop();
    evidence.push(LegacyDeliveryStatus::new(
        "a-retained",
        address("b"),
        TaskStatus::Completed,
    ));
    refused(EmbeddedStore::open_with_clock_and_idempotency_metadata(
        &fixture.path,
        clock.clone(),
        &owners,
        &evidence,
    ));
    assert_eq!(
        snapshot(&fixture.path),
        before,
        "status evidence cannot override resolved owner"
    );
    evidence.pop();
    // Two retained rows with the same owner/full status prove one value.
    let store = EmbeddedStore::open_with_clock_and_idempotency_metadata(
        &fixture.path,
        clock,
        &owners,
        &evidence,
    )
    .expect("coherent repeated retained Failed proof");
    drop(store);
    let after = snapshot(&fixture.path);
    assert_eq!(after["mb_entries"], before["mb_entries"]);
    fixture.assert_backup_untouched();
}

#[tokio::test]
async fn duplicate_status_survives_consume_restart_and_original_expiry() {
    let dir = tempfile::tempdir().expect("isolated status file");
    let path = dir.path().join("status.redb");
    let clock = TestClock::new(u128::from(NOW));
    let original = TaskStatus::Failed(TaskError::transient("first attempt failed in device 7"));
    {
        let store = EmbeddedStore::open_with_clock(&path, clock.clone()).expect("fresh schema3");
        let mailbox = store.result_mailbox::<String>();
        let first = Delivery::new(
            address("a"),
            1,
            original.clone(),
            Some("original result".to_owned()),
        )
        .with_idempotency_key("once")
        .with_ttl(Duration::from_secs(1));
        assert_eq!(
            mailbox.deliver(first).await.expect("first failure"),
            DeliveryOutcome::Stored
        );
        assert_eq!(
            mailbox
                .deliver(retry(address("a"), "once"))
                .await
                .expect("local success retry"),
            DeliveryOutcome::Duplicate {
                status: original.clone()
            },
            "OA-07:stored-status: retry hid durable failure"
        );
        let consumed = mailbox
            .consume(&address("a"))
            .await
            .expect("client consume")
            .expect("one original");
        assert_eq!(consumed.status, original);
        assert_eq!(consumed.payload.as_deref(), Some("original result"));
    }
    let store =
        EmbeddedStore::open_with_clock(&path, clock.clone()).expect("restart consumed status");
    let mailbox = store.result_mailbox::<String>();
    clock.advance(Duration::from_millis(999));
    assert_eq!(
        mailbox
            .deliver(retry(address("a"), "once"))
            .await
            .expect("consumed retry"),
        DeliveryOutcome::Duplicate {
            status: original.clone()
        },
        "OA-07:consumed-status: restart forgot original failure"
    );
    assert!(
        mailbox
            .peek(&address("a"))
            .await
            .expect("no second result")
            .is_none()
    );
    clock.advance(Duration::from_millis(1));
    assert_eq!(
        mailbox
            .evict_expired()
            .await
            .expect("expire consumed metadata"),
        0
    );
    assert_eq!(
        mailbox
            .deliver(retry(address("a"), "once"))
            .await
            .expect("new generation"),
        DeliveryOutcome::Stored
    );
    let failed_retry =
        Delivery::new(address("a"), 100, original, None::<String>).with_idempotency_key("once");
    assert_eq!(
        mailbox
            .deliver(failed_retry)
            .await
            .expect("new first status"),
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
}

#[tokio::test]
async fn malformed_authoritative_status_is_an_error_without_consuming_result() {
    let dir = tempfile::tempdir().expect("isolated malformed-metadata fixture");
    let path = dir.path().join("malformed.redb");
    let clock = TestClock::new(u128::from(NOW));
    {
        let store = EmbeddedStore::open_with_clock(&path, clock.clone()).expect("new store");
        assert_eq!(
            store
                .result_mailbox::<String>()
                .deliver(retry(address("a"), "malformed"))
                .await
                .expect("first result"),
            DeliveryOutcome::Stored
        );
    }
    {
        let db = Database::open(&path).expect("owned corruption fixture");
        let txn = db.begin_write().expect("corruption transaction");
        let table: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("mb_idem_v3");
        let key = serde_json::to_string(&address("a")).expect("address JSON");
        txn.open_table(table)
            .expect("active metadata")
            .insert(
                (key.as_str(), "malformed"),
                b"{\"forget_at_ms\":18446744073709551615}".as_slice(),
            )
            .expect("missing status fixture");
        txn.commit().expect("owned malformed record");
    }
    let store = EmbeddedStore::open_with_clock(&path, clock).expect("reopen malformed record");
    let mailbox = store.result_mailbox::<String>();
    assert!(
        matches!(
            mailbox.deliver(retry(address("a"), "malformed")).await,
            Err(SchedulerError::Backend { .. })
        ),
        "missing status cannot be a successful duplicate"
    );
    assert_eq!(
        mailbox
            .peek(&address("a"))
            .await
            .expect("original remains")
            .expect("original result")
            .status,
        TaskStatus::Completed
    );
}
