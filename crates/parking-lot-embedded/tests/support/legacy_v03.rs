//! Independent schema-1 fixture protocol, frozen from release baseline
//! 92da0d3d28f7896ddc67062fa49ba40ea48d74a3. It uses raw redb transactions,
//! never the new `EmbeddedStore` or mailbox migration to generate legacy data.
//! This is an on-disk protocol compatibility probe, not a published old binary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use prometheus_parking_lot_core::{LegacyIdempotencyOwner, MailboxKey};
use redb::{
    Database, ReadableDatabase, ReadableTable, TableDefinition, TableHandle, WriteTransaction,
};
use serde_json::{Value, json};

pub const NOW: u64 = 1_700_000_000_000;
pub const IDENTITY: &str = "redb:00000000000000110000000000000022";
pub const ENTRIES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("mb_entries");
pub const IDEM: TableDefinition<&str, u64> = TableDefinition::new("mb_idem");
pub const MB_META: TableDefinition<&str, u64> = TableDefinition::new("mb_meta");
pub const Q_META: TableDefinition<&str, u64> = TableDefinition::new("q_meta");
const TASKS: TableDefinition<u64, &[u8]> = TableDefinition::new("q_tasks");
const STATE: TableDefinition<u64, (u8, u8, u64, u64, u32)> = TableDefinition::new("q_state");
const AVAIL: TableDefinition<(u8, u64), ()> = TableDefinition::new("q_avail");
const DELAYED: TableDefinition<(u64, u64), ()> = TableDefinition::new("q_delayed");
const LEASED: TableDefinition<(u64, u64), ()> = TableDefinition::new("q_leased");
const Q_IDEM: TableDefinition<&str, u64> = TableDefinition::new("q_idem");
const Q_IDEM_REV: TableDefinition<u64, &str> = TableDefinition::new("q_idem_rev");
const DEAD: TableDefinition<u64, &[u8]> = TableDefinition::new("q_dead");
const SCOPED: TableDefinition<(&str, &str), u64> = TableDefinition::new("mb_idem_v2");
const OWNERS: TableDefinition<&str, &str> = TableDefinition::new("mb_legacy_owners_v2");

pub fn address(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.to_owned(),
        user_id: Some("user".to_owned()),
        session_id: Some("session".to_owned()),
    }
}

pub fn assignments() -> Vec<LegacyIdempotencyOwner> {
    vec![
        LegacyIdempotencyOwner::new("b-consumed-finite", address("a")),
        LegacyIdempotencyOwner::new("c-consumed-permanent", address("b")),
    ]
}

/// Deliberately noncryptographic content fingerprint. The final boundary
/// runner binds fixture/backup SHA256 separately; this is not a security hash.
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("fnv1a64:{hash:016x}")
}

pub struct Fixture {
    pub path: PathBuf,
    pub backup: PathBuf,
    original_backup: Vec<u8>,
}

impl Fixture {
    pub fn new(ambiguous: bool) -> Self {
        let dir = tempfile::tempdir().expect("isolated fixture directory");
        let path = dir.path().join("working.redb");
        let backup = dir.path().join("untouched-v03.redb");
        seed(&path, ambiguous);
        fs::copy(&path, &backup).expect("untouched closed legacy backup");
        let original_backup = fs::read(&backup).expect("backup bytes");
        let manifest = json!({
            "source_commit": "92da0d3d28f7896ddc67062fa49ba40ea48d74a3",
            "source_sha256": {
                "crates/parking-lot-embedded/src/store.rs": "2121bf82d61f2c7b75f86245e4baa7d927f14a0cf708c0f307518321548057c5",
                "crates/parking-lot-embedded/src/mailbox.rs": "68b4e09b001ffbecf8c285f014e1bab63df9ebef3d58f01a8719d270d1934c74",
                "crates/parking-lot-embedded/src/queue.rs": "3b8dde84e633deee3870da71104e2464ecc6dbbb1e0795eb82e3a662f2471743"
            },
            "schema": 1,
            "redb_version": "4.4.0",
            "clock_ms": NOW,
            "queue_identity": IDENTITY,
            "expiry_precision": "whole wall-clock milliseconds; MAX=permanent",
            "working": path,
            "untouched_backup": backup,
            "backup_fingerprint_noncryptographic": fingerprint(&original_backup),
            "backup_bytes": original_backup.len(),
            "final_packet_sha256": "NOT_EXERCISED; boundary runner must bind both files",
            "ambiguous_generation": ambiguous,
            "authoritative_test_owners": assignments(),
            "legacy_logical_snapshot": snapshot(&path)
        });
        let manifest_path = dir.path().join("fixture-manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("fixture manifest");
        eprintln!("OA-06 redb fixture retained at {}", manifest_path.display());
        // Deliberate evidence retention on success and failure. The boundary
        // runner captures SHA256/logs, then cleans only this owned directory.
        std::mem::forget(dir);
        Self {
            path,
            backup,
            original_backup,
        }
    }

    pub fn assert_backup_untouched(&self) {
        assert_eq!(
            fs::read(&self.backup).expect("backup bytes"),
            self.original_backup
        );
    }

    pub fn restore_copy(&self) -> PathBuf {
        let restored = self.path.with_file_name("restored-v03.redb");
        fs::copy(&self.backup, &restored).expect("restore isolated backup copy");
        restored
    }
}

/// Original schema-1 create/check protocol from store.rs and the table lists
/// from queue.rs/mailbox.rs. In particular it rejects Some(version != 1)
/// before commit, as the unmodified 0.3 initializer does.
pub fn old_open(path: &Path) -> Result<Database, String> {
    let db = Database::create(path).map_err(|e| e.to_string())?;
    let txn = db.begin_write().map_err(|e| e.to_string())?;
    macro_rules! create {
        ($($table:expr),+ $(,)?) => { $(txn.open_table($table).map_err(|e| e.to_string())?;)+ };
    }
    create!(
        TASKS, STATE, AVAIL, DELAYED, LEASED, Q_IDEM, Q_IDEM_REV, DEAD, Q_META
    );
    create!(ENTRIES, IDEM, MB_META);
    {
        let mut meta = txn.open_table(Q_META).map_err(|e| e.to_string())?;
        let version = meta
            .get("schema_version")
            .map_err(|e| e.to_string())?
            .map(|v| v.value());
        match version {
            None => {
                meta.insert("schema_version", 1)
                    .map_err(|e| e.to_string())?;
            }
            Some(1) => {}
            Some(other) => {
                return Err(format!(
                    "embedded store has layout version {other}, this build understands 1"
                ));
            }
        }
    }
    txn.commit().map_err(|e| e.to_string())?;
    Ok(db)
}

fn entry(task: u64, caller: Option<&str>, payload: &str, expires: Option<u64>) -> Vec<u8> {
    let delivered_at = match task {
        3 => NOW - 10_000,
        4 => NOW,
        _ => NOW - 100,
    };
    serde_json::to_vec(&json!({
        "task_id": task, "status": "Completed", "payload": payload,
        "idempotency_key": caller, "delivered_at_ms": delivered_at,
        "expires_at_ms": expires
    }))
    .expect("legacy entry JSON")
}

fn seed(path: &Path, ambiguous: bool) {
    let db = old_open(path).expect("legacy schema-1 open");
    let txn = db.begin_write().expect("legacy population transaction");
    seed_queue(&txn);
    seed_mailbox(&txn, ambiguous);
    txn.commit().expect("legacy fixture durable commit");
}

fn seed_queue(txn: &WriteTransaction) {
    let mut meta = txn.open_table(Q_META).expect("queue meta");
    for (name, value) in [
        ("store_id_hi", 0x11),
        ("store_id_lo", 0x22),
        ("next_id", 5),
        ("next_fence", 18),
    ] {
        meta.insert(name, value).expect("queue counter");
    }
    let mut tasks = txn.open_table(TASKS).expect("tasks");
    let mut state = txn.open_table(STATE).expect("state");
    let mut idem = txn.open_table(Q_IDEM).expect("queue idem");
    let mut reverse = txn.open_table(Q_IDEM_REV).expect("queue reverse idem");
    for (id, key, kind, priority, time, fence, attempt) in [
        (1, "ready", 0, 253, 0, 0, 0),
        (2, "delayed", 1, 254, NOW + 1_000, 16, 1),
        (3, "leased", 2, 255, NOW + 1_000, 17, 2),
    ] {
        let priority_name = match key {
            "ready" => "high",
            "delayed" => "normal",
            _ => "low",
        };
        let task = serde_json::to_vec(&json!({
            "meta": {"id": id, "mailbox": null, "priority": priority_name, "cost": {"cpu": 1},
                     "deadline_ms": null, "created_at_ms": NOW - 100, "idempotency_key": key,
                     "sequence": id}, "payload": key
        }))
        .expect("legacy task JSON");
        tasks.insert(id, task.as_slice()).expect("task");
        state
            .insert(id, (kind, priority, time, fence, attempt))
            .expect("task state");
        idem.insert(key, id).expect("queue idempotency");
        reverse.insert(id, key).expect("queue reverse idempotency");
    }
    txn.open_table(AVAIL)
        .expect("available")
        .insert((253, 1), ())
        .expect("ready index");
    txn.open_table(DELAYED)
        .expect("delayed")
        .insert((NOW + 1_000, 2), ())
        .expect("delayed index");
    txn.open_table(LEASED)
        .expect("leased")
        .insert((NOW + 1_000, 3), ())
        .expect("leased index");
    let dead = serde_json::to_vec(&json!({
        "task": {"meta": {"id": 4, "mailbox": null, "priority": "low", "cost": {"cpu": 1},
                          "deadline_ms": null, "created_at_ms": NOW - 100,
                          "idempotency_key": "dead", "sequence": 4}, "payload": "dead"},
        "attempts": 3, "reason": "legacy-dead"
    }))
    .expect("legacy dead letter JSON");
    txn.open_table(DEAD)
        .expect("dead")
        .insert(4, dead.as_slice())
        .expect("dead letter");
}

fn seed_mailbox(txn: &WriteTransaction, ambiguous: bool) {
    let a = serde_json::to_string(&address("a")).expect("address a");
    let b = serde_json::to_string(&address("b")).expect("address b");
    let mut entries = txn.open_table(ENTRIES).expect("mailbox entries");
    for (owner, sequence, caller, payload, expiry) in [
        (a.as_str(), 1, Some("a-retained"), "legacy first", None),
        (a.as_str(), 2, None, "unkeyed", None),
        (
            b.as_str(),
            3,
            Some("d-reused"),
            "old generation",
            Some(NOW - 1),
        ),
        (
            a.as_str(),
            4,
            Some("d-reused"),
            "new generation",
            Some(NOW + 5_000),
        ),
        (
            a.as_str(),
            5,
            Some("e-live-finite"),
            "finite live",
            Some(NOW + 10_000),
        ),
    ] {
        entries
            .insert(
                (owner, sequence),
                entry(sequence, caller, payload, expiry).as_slice(),
            )
            .expect("legacy result");
    }
    let mut fences = txn.open_table(IDEM).expect("legacy fences");
    for (caller, expiry) in [
        ("a-retained", u64::MAX),
        ("b-consumed-finite", NOW + 1_000),
        ("c-consumed-permanent", u64::MAX),
        ("d-reused", NOW + 5_000),
        ("e-live-finite", NOW + 10_000),
        ("f-expired-ownerless", NOW - 1),
    ] {
        fences
            .insert(caller, expiry)
            .expect("legacy remembered fence");
    }
    if ambiguous {
        for (owner, sequence) in [(a.as_str(), 6), (b.as_str(), 7)] {
            entries
                .insert(
                    (owner, sequence),
                    entry(
                        sequence,
                        Some("g-ambiguous"),
                        "ambiguous",
                        Some(NOW + 2_000),
                    )
                    .as_slice(),
                )
                .expect("ambiguous retained row");
        }
        fences
            .insert("g-ambiguous", NOW + 2_000)
            .expect("ambiguous fence");
    }
    txn.open_table(MB_META)
        .expect("mailbox meta")
        .insert("next_sequence", if ambiguous { 8 } else { 6 })
        .expect("mailbox sequence");
}

/// Logical rows, including raw payload bytes and all queue indexes/counters.
/// Optional new tables and table names detect partial migration on refusal.
pub fn snapshot(path: &Path) -> BTreeMap<String, Value> {
    let db = Database::open(path).expect("snapshot open");
    let txn = db.begin_read().expect("snapshot read");
    let mut out = BTreeMap::new();
    let names: Vec<_> = txn
        .list_tables()
        .expect("table names")
        .map(|table| table.name().to_owned())
        .collect();
    out.insert("tables".to_owned(), json!(names));
    macro_rules! rows {
        ($($table:expr),+ $(,)?) => { $(
            if names.iter().any(|name| name == $table.name()) {
                let table = txn.open_table($table).expect("snapshot table");
                let rows: Vec<_> = table.iter().expect("snapshot rows").map(|row| {
                    let (key, value) = row.expect("snapshot row");
                    json!([key.value(), value.value()])
                }).collect();
                out.insert($table.name().to_owned(), json!(rows));
            }
        )+ };
    }
    rows!(
        TASKS, STATE, AVAIL, DELAYED, LEASED, Q_IDEM, Q_IDEM_REV, DEAD, Q_META, ENTRIES, IDEM,
        MB_META, SCOPED, OWNERS
    );
    out
}
