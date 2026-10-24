//! OA-07 legacy metadata fixtures. Schema 1 starts from the frozen OA-06
//! backup protocol; schema 2 is populated independently in raw old-format
//! tables, never by opening schema 3 and changing its marker.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use prometheus_parking_lot_core::{LegacyDeliveryStatus, TaskError, TaskStatus};
use redb::{Database, ReadableDatabase, ReadableTable, Table, TableDefinition};
use serde_json::{Value, json};

use super::legacy_v03::{self, ENTRIES, IDEM, MB_META, NOW, Q_META, address};

const SCOPED: TableDefinition<(&str, &str), u64> = TableDefinition::new("mb_idem_v2");
const OWNERS: TableDefinition<&str, &str> = TableDefinition::new("mb_legacy_owners_v2");
const ACTIVE: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("mb_idem_v3");
const METADATA: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("mb_legacy_metadata_v3");

pub fn retained_failure() -> TaskStatus {
    TaskStatus::Failed(TaskError::transient("original retained failure"))
}

pub fn finite_failure() -> TaskStatus {
    TaskStatus::Failed(TaskError::executor("original consumed finite failure"))
}

pub const fn permanent_failure() -> TaskStatus {
    TaskStatus::Failed(TaskError::Timeout {
        budget: Duration::from_millis(37),
    })
}

pub fn statuses(schema: u64) -> Vec<LegacyDeliveryStatus> {
    let mut statuses = vec![
        LegacyDeliveryStatus::new("b-consumed-finite", address("a"), finite_failure()),
        LegacyDeliveryStatus::new("c-consumed-permanent", address("b"), permanent_failure()),
    ];
    if schema == 2 {
        statuses.extend([
            LegacyDeliveryStatus::new("z-consumed", address("a"), TaskStatus::Completed),
            LegacyDeliveryStatus::new(
                "z-consumed",
                address("b"),
                TaskStatus::Failed(TaskError::Cancelled {
                    reason: "original shutdown".to_owned(),
                }),
            ),
        ]);
    }
    statuses
}

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("fnv1a64:{hash:016x}")
}

pub struct MetadataFixture {
    pub path: PathBuf,
    pub backup: PathBuf,
    original_backup: Vec<u8>,
}

impl MetadataFixture {
    pub fn new(schema: u64, ambiguous: bool) -> Self {
        assert!(matches!(schema, 1 | 2));
        let source = legacy_v03::Fixture::new(false);
        let dir = tempfile::tempdir().expect("isolated metadata fixture");
        let path = dir.path().join("working.redb");
        let backup = dir.path().join(format!("untouched-schema{schema}.redb"));
        fs::copy(&source.backup, &path).expect("independent legacy source copy");
        populate(&path, schema, ambiguous);
        source.assert_backup_untouched();
        fs::copy(&path, &backup).expect("closed source metadata backup");
        let original_backup = fs::read(&backup).expect("metadata backup bytes");
        let manifest = json!({
            "source_schema": schema,
            "source_v03_commit": "92da0d3d28f7896ddc67062fa49ba40ea48d74a3",
            "source_v03_backup": source.backup,
            "intermediate_oa06_source_sha256": {
                "crates/parking-lot-embedded/src/mailbox.rs": "c09b9cfc0a4ee9dd891f77fc96b8665d31061cd8829b18f25cf459814d6be12e",
                "crates/parking-lot-embedded/src/store.rs": "bd6a5d2fd171d62059aa615cd6342946193f0ebcc78a41743e17df90b210984a"
            },
            "fixture_protocol": "raw legacy tables; no schema3 generation or marker relabel",
            "clock_ms": NOW, "redb_version": "4.4.0",
            "expiry_precision": "original wall milliseconds; MAX=permanent",
            "working": path, "untouched_backup": backup,
            "backup_fingerprint_noncryptographic": fingerprint(&original_backup),
            "backup_bytes": original_backup.len(),
            "final_packet_sha256": "NOT_EXERCISED; runner must bind retained files",
            "authoritative_statuses": statuses(schema),
            "authority": "synthetic original outcomes for this isolated fixture only; not retry outcomes",
            "ambiguous_retained_owner_status_pairs": ambiguous,
            "logical_source": snapshot(&path)
        });
        let manifest_path = dir.path().join("metadata-fixture-manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("metadata fixture manifest");
        eprintln!("OA-07 redb fixture retained at {}", manifest_path.display());
        std::mem::forget(dir);
        Self {
            path,
            backup,
            original_backup,
        }
    }

    pub fn assert_backup_untouched(&self) {
        assert_eq!(
            fs::read(&self.backup).expect("metadata backup bytes"),
            self.original_backup
        );
    }

    pub fn restore_copy(&self) -> PathBuf {
        let restored = self.path.with_file_name("restored-source.redb");
        fs::copy(&self.backup, &restored).expect("restore untouched backup copy");
        restored
    }
}

fn populate_original_statuses(entries: &mut Table<'_, (&str, u64), &[u8]>) {
    let mut replacements = Vec::new();
    for row in entries.iter().expect("legacy rows") {
        let (key, bytes) = row.expect("legacy row");
        let (owner, seq) = key.value();
        let mut value: Value = serde_json::from_slice(bytes.value()).expect("legacy JSON");
        let status = match seq {
            1 => retained_failure(),
            3 => TaskStatus::Failed(TaskError::executor("expired earlier generation")),
            4 => TaskStatus::Running,
            5 => TaskStatus::Queued,
            _ => TaskStatus::Completed,
        };
        value["status"] = serde_json::to_value(status).expect("status JSON");
        replacements.push((
            owner.to_owned(),
            seq,
            serde_json::to_vec(&value).expect("entry JSON"),
        ));
    }
    for (owner, seq, bytes) in &replacements {
        entries
            .insert((owner.as_str(), *seq), bytes.as_slice())
            .expect("original fixture status");
    }
}

fn populate(path: &Path, schema: u64, ambiguous: bool) {
    let db = Database::open(path).expect("raw source open");
    let txn = db.begin_write().expect("raw source transaction");
    {
        let mut entries = txn.open_table(ENTRIES).expect("legacy entries");
        populate_original_statuses(&mut entries);
        let a = serde_json::to_string(&address("a")).expect("a");
        let b = serde_json::to_string(&address("b")).expect("b");
        if schema == 2 {
            // The intermediate writer stores only scoped expiry, with no
            // status column or schema-3 tables. Equal caller keys may coexist.
            let mut scoped = txn.open_table(SCOPED).expect("old scoped fences");
            let mut owners = txn.open_table(OWNERS).expect("old owner provenance");
            let legacy = txn.open_table(IDEM).expect("legacy global provenance");
            for row in legacy.iter().expect("legacy fences") {
                let (caller, expiry) = row.expect("legacy fence");
                if expiry.value() <= NOW {
                    continue;
                }
                let owner = if caller.value() == "c-consumed-permanent" {
                    b.as_str()
                } else {
                    a.as_str()
                };
                scoped
                    .insert((owner, caller.value()), expiry.value())
                    .expect("schema2 scoped row");
                owners
                    .insert(caller.value(), owner)
                    .expect("immutable original owner");
            }
            for (owner, seq, status) in [
                (a.as_str(), 6, TaskStatus::Expired),
                (
                    b.as_str(),
                    7,
                    TaskStatus::Dropped("original drop".to_owned()),
                ),
            ] {
                let bytes = serde_json::to_vec(&json!({
                    "task_id": seq, "status": status, "payload": "scoped original",
                    "idempotency_key": "same-scoped", "delivered_at_ms": NOW,
                    "expires_at_ms": NOW + 2_000
                }))
                .expect("scoped result");
                entries
                    .insert((owner, seq), bytes.as_slice())
                    .expect("old scoped entry");
                scoped
                    .insert((owner, "same-scoped"), NOW + 2_000)
                    .expect("old scoped fence");
                scoped
                    .insert((owner, "z-consumed"), NOW + 4_000)
                    .expect("old consumed scoped fence");
            }
            scoped
                .insert((b.as_str(), "v2-expired"), NOW - 1)
                .expect("expired source provenance");
        }
        if ambiguous {
            // Adversarial legacy rows: selecting owner a and status Completed
            // from owner b would mix independent facts and must be refused.
            for (owner, seq, status) in [
                (b.as_str(), 8, TaskStatus::Completed),
                (a.as_str(), 9, retained_failure()),
            ] {
                let bytes = serde_json::to_vec(&json!({
                    "task_id": seq, "status": status, "payload": "ambiguous proof",
                    "idempotency_key": "a-retained", "delivered_at_ms": NOW - 100,
                    "expires_at_ms": null
                }))
                .expect("ambiguous entry");
                entries
                    .insert((owner, seq), bytes.as_slice())
                    .expect("ambiguous retained metadata");
            }
        }
        txn.open_table(MB_META)
            .expect("mailbox meta")
            .insert(
                "next_sequence",
                if ambiguous {
                    10
                } else if schema == 2 {
                    8
                } else {
                    6
                },
            )
            .expect("legacy order");
        txn.open_table(Q_META)
            .expect("queue metadata")
            .insert("schema_version", schema)
            .expect("source schema");
    }
    txn.commit().expect("raw source fixture commit");
}

/// Frozen OA-06 ordinary-open protocol rejects every version other than 2.
/// This is a real redb protocol probe, not execution of a released old binary.
pub fn old_v2_open(path: &Path) -> Result<Database, String> {
    let db = Database::open(path).map_err(|e| e.to_string())?;
    let txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.open_table(SCOPED).map_err(|e| e.to_string())?;
    txn.open_table(OWNERS).map_err(|e| e.to_string())?;
    let version = {
        let meta = txn.open_table(Q_META).map_err(|e| e.to_string())?;
        meta.get("schema_version")
            .map_err(|e| e.to_string())?
            .map(|version| version.value())
    };
    if version != Some(2) {
        return Err(format!(
            "embedded store layout {version:?}, this build understands 2"
        ));
    }
    txn.commit().map_err(|e| e.to_string())?;
    Ok(db)
}

pub fn snapshot(path: &Path) -> BTreeMap<String, Value> {
    let mut out = legacy_v03::snapshot(path);
    let names = out["tables"].as_array().expect("table names");
    let has_active = names.iter().any(|name| name == "mb_idem_v3");
    let has_metadata = names.iter().any(|name| name == "mb_legacy_metadata_v3");
    let db = Database::open(path).expect("metadata snapshot open");
    let txn = db.begin_read().expect("metadata snapshot read");
    for (name, definition, present) in [
        ("mb_idem_v3", ACTIVE, has_active),
        ("mb_legacy_metadata_v3", METADATA, has_metadata),
    ] {
        if present {
            let rows: Vec<_> = txn
                .open_table(definition)
                .expect("metadata table")
                .iter()
                .expect("metadata rows")
                .map(|row| {
                    let (key, value) = row.expect("metadata row");
                    json!([key.value(), value.value()])
                })
                .collect();
            out.insert(name.to_owned(), json!(rows));
        }
    }
    out
}
