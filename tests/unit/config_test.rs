//! Scheduler configuration: schema strictness, validation, documented examples
//! (cr-37, scenario S-22).
//!
//! Every test is named `config_*` so the S-22 entry point selects them:
//! `cargo nextest run -p prometheus_parking_lot --all-features --locked -E "test(config_)"`.
//!
//! Negative control: built with `--cfg parking_lot_negative_control="s-22"` the
//! config structs stop denying unknown fields, and the `*_misspelled_*` tests
//! below must fail.

use std::collections::BTreeMap;
use std::path::PathBuf;

use prometheus_parking_lot::util::ResourceKind;

use prometheus_parking_lot::config::{
    BackendConfig, ConfigError, PoolConfig, RedbBackendConfig, SchedulerConfig, StorageConfig,
    WorkerPoolConfig,
};

const fn pool(max_units: u32) -> PoolConfig {
    PoolConfig {
        max_units,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: StorageConfig::InMemory,
        mailbox: StorageConfig::InMemory,
        max_head_skips: 8,
        resource_limits: BTreeMap::new(),
    }
}

fn one_pool(pool: PoolConfig) -> SchedulerConfig {
    SchedulerConfig {
        backends: BTreeMap::new(),
        pools: BTreeMap::from([("inference".to_owned(), pool)]),
    }
}

/// The parse error for `json`, which must not load.
fn parse_error(json: &str) -> String {
    match SchedulerConfig::from_json_str(json) {
        Ok(cfg) => panic!("expected a parse error, loaded {cfg:?}"),
        Err(err @ ConfigError::Parse(_)) => err.to_string(),
        Err(other) => panic!("expected a parse error, got {other}"),
    }
}

/// The field named by the validation error for `json`.
fn invalid_field(json: &str) -> String {
    match SchedulerConfig::from_json_str(json) {
        Ok(cfg) => panic!("expected a validation error, loaded {cfg:?}"),
        Err(err) => err
            .field()
            .unwrap_or_else(|| panic!("expected a validation error, got {err}"))
            .to_owned(),
    }
}

const POOL: &str = r#""max_units": 4, "max_queue_depth": 8, "default_timeout_secs": 30,
                      "queue": "in_memory", "mailbox": "in_memory""#;

// ── strict schema: misspelled fields fail, naming the field ─────────────────

#[test]
fn config_misspelled_pool_field_is_rejected_naming_it() {
    let json = r#"{ "pools": { "p": { "max_units": 4, "max_queue_dept": 8,
        "default_timeout_secs": 30, "queue": "in_memory", "mailbox": "in_memory",
        "max_queue_depth": 8 } } }"#;
    let message = parse_error(json);
    assert!(
        message.contains("unknown field `max_queue_dept`"),
        "error must name the misspelled field: {message}"
    );
}

#[test]
fn config_misspelled_top_level_field_is_rejected_naming_it() {
    let json = format!(r#"{{ "backend": {{}}, "pools": {{ "p": {{ {POOL} }} }} }}"#);
    let message = parse_error(&json);
    assert!(
        message.contains("unknown field `backend`"),
        "error must name the misspelled field: {message}"
    );
}

#[test]
fn config_misspelled_postgres_backend_field_is_rejected_naming_it() {
    let json = format!(
        r#"{{ "backends": {{ "db": {{ "type": "postgres", "url": "postgres://u@h/d",
            "queue_name": "q", "max_depth": 10, "lease_secs": 30, "listne": true }} }},
            "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let message = parse_error(&json);
    assert!(
        message.contains("unknown field `listne`"),
        "error must name the misspelled field: {message}"
    );
}

#[test]
fn config_misspelled_redb_backend_field_is_rejected_naming_it() {
    let json = format!(
        r#"{{ "backends": {{ "local": {{ "type": "redb", "path": "./q.redb",
            "max_depth": 10, "lease_secs": 30, "fsync": false }} }},
            "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let message = parse_error(&json);
    assert!(
        message.contains("unknown field `fsync`"),
        "error must name the misspelled field: {message}"
    );
}

#[test]
fn config_misspelled_worker_pool_field_is_rejected_naming_it() {
    let err = serde_json::from_str::<WorkerPoolConfig>(r#"{ "worker_cout": 4 }"#)
        .expect_err("a misspelled field must not load");
    assert!(
        err.to_string().contains("unknown field `worker_cout`"),
        "error must name the misspelled field: {err}"
    );
}

#[test]
fn config_misspelled_0_2_runtime_field_is_rejected_instead_of_ignored() {
    // 0.2 accepted `"runtime"` and ignored it; it now fails, naming the field.
    let json = format!(r#"{{ "pools": {{ "p": {{ {POOL}, "runtime": "native" }} }} }}"#);
    let message = parse_error(&json);
    assert!(
        message.contains("unknown field `runtime`"),
        "error must name the field: {message}"
    );
}

#[test]
fn config_unknown_backend_type_is_rejected_naming_the_type() {
    let json = format!(
        r#"{{ "backends": {{ "q": {{ "type": "sqlite", "queue_name": "q" }} }},
            "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let message = parse_error(&json);
    assert!(
        message.contains("unknown variant `sqlite`"),
        "error must name the type: {message}"
    );
}

#[test]
fn config_worker_pool_config_loads_every_field_on_every_target() {
    let cfg: WorkerPoolConfig = serde_json::from_str(
        r#"{ "worker_count": 2, "thread_stack_size": 1048576, "max_units": 10,
             "max_queue_depth": 20, "default_timeout_ms": 5000 }"#,
    )
    .expect("a complete worker pool config loads");
    assert_eq!(cfg.thread_stack_size, 1_048_576);
    assert!(cfg.validate().is_ok());
}

// ── validation names the offending field ────────────────────────────────────

#[test]
fn config_pool_limits_are_validated() {
    assert!(pool(100).validate().is_ok());
    let zero_units = pool(0).validate().expect_err("zero units");
    assert_eq!(zero_units.field(), Some("max_units"));

    let mut zero_depth = pool(1);
    zero_depth.max_queue_depth = 0;
    assert_eq!(
        zero_depth.validate().expect_err("zero depth").field(),
        Some("max_queue_depth")
    );

    let mut zero_timeout = pool(1);
    zero_timeout.default_timeout_secs = 0;
    assert_eq!(
        zero_timeout.validate().expect_err("zero timeout").field(),
        Some("default_timeout_secs")
    );
}

/// cr-23: per-dimension limits load from JSON, are optional (a 0.2-shaped
/// pool loads unchanged) and are validated per kind by dotted path.
#[test]
fn config_resource_limits_load_and_are_validated_per_kind() {
    let cfg = SchedulerConfig::from_json_str(
        r#"{ "pools": { "inference": {
            "max_units": 32, "max_queue_depth": 10, "default_timeout_secs": 60,
            "queue": "in_memory", "mailbox": "in_memory",
            "resource_limits": { "gpu_vram": 24, "cpu": 4 } } } }"#,
    )
    .expect("multi-dimension limits load");
    let limits = &cfg.pools["inference"].resource_limits;
    assert_eq!(limits.get(&ResourceKind::GpuVram), Some(&24));
    assert_eq!(limits.get(&ResourceKind::Cpu), Some(&4));
    let json = serde_json::to_string(&cfg.pools["inference"]).expect("serialize");
    assert!(
        json.contains(r#""resource_limits":{"cpu":4,"gpu_vram":24}"#),
        "{json}"
    );
    assert!(
        !serde_json::to_string(&pool(8))
            .expect("serialize")
            .contains("resource_limits"),
        "an empty map is omitted, so 0.2-shaped output is unchanged"
    );

    let mut zero = pool(8);
    zero.resource_limits.insert(ResourceKind::Cpu, 0);
    let err = one_pool(zero).validate().expect_err("zero limit");
    assert_eq!(err.field(), Some("pools.inference.resource_limits.cpu"));

    let mut unreachable = pool(8);
    unreachable.resource_limits.insert(ResourceKind::GpuVram, 9);
    let err = unreachable.validate().expect_err("limit above max_units");
    assert_eq!(err.field(), Some("resource_limits.gpu_vram"));
    assert!(err.to_string().contains("exceeds max_units (8)"), "{err}");

    let message = parse_error(
        r#"{ "pools": { "p": { "max_units": 8, "max_queue_depth": 1, "default_timeout_secs": 1,
            "queue": "in_memory", "mailbox": "in_memory",
            "resource_limits": { "tpu": 1 } } } }"#,
    );
    assert!(message.contains("unknown variant `tpu`"), "{message}");
}

#[test]
fn config_worker_pool_resource_limits_load_and_are_validated() {
    let cfg: WorkerPoolConfig = serde_json::from_str(
        r#"{ "max_units": 16, "resource_limits": { "gpu_vram": 16, "cpu": 2 } }"#,
    )
    .expect("worker pool limits load");
    assert_eq!(cfg.resource_limits.get(&ResourceKind::Cpu), Some(&2));
    assert!(cfg.validate().is_ok());
    let over = WorkerPoolConfig::new()
        .with_max_units(4)
        .with_resource_limit(ResourceKind::GpuVram, 5);
    assert!(
        over.validate()
            .expect_err("over max_units")
            .contains("exceeds max_units")
    );
    let zero = WorkerPoolConfig::new().with_resource_limit(ResourceKind::Io, 0);
    assert!(
        zero.validate()
            .expect_err("zero")
            .contains("greater than 0")
    );
}

#[test]
fn config_scheduler_errors_carry_the_dotted_path() {
    let err = one_pool(pool(0)).validate().expect_err("zero units");
    assert_eq!(err.field(), Some("pools.inference.max_units"));
    assert_eq!(
        err.to_string(),
        "pools.inference.max_units: must be greater than 0"
    );

    let empty = SchedulerConfig {
        backends: BTreeMap::new(),
        pools: BTreeMap::new(),
    };
    assert_eq!(
        empty.validate().expect_err("no pools").field(),
        Some("pools")
    );
}

#[test]
fn config_pool_on_unknown_backend_is_rejected() {
    let json = r#"{ "pools": { "p": { "max_units": 4, "max_queue_depth": 8,
        "default_timeout_secs": 30, "queue": { "backend": "nope" }, "mailbox": "in_memory" } } }"#;
    assert_eq!(invalid_field(json), "pools.p.queue");
}

/// `one_pool(pool(4))` plus redb backends named `names` (never opened).
fn with_redb_backends(names: &[&str]) -> SchedulerConfig {
    let mut cfg = one_pool(pool(4));
    for name in names {
        cfg.backends.insert(
            (*name).to_owned(),
            BackendConfig::Redb(RedbBackendConfig {
                path: PathBuf::from(format!("./never-opened-{name}.redb")),
                max_depth: 10,
                lease_secs: 30,
                max_attempts: None,
            }),
        );
    }
    cfg
}

fn on(backend: &str) -> StorageConfig {
    StorageConfig::Backend(backend.to_owned())
}

#[test]
fn config_pool_on_a_durable_backend_needs_queue_and_mailbox_on_it() {
    // Durable dispatch (cr-49): a pool runs on a backend only when its queue
    // and its mailbox both name that backend; half in memory would lose
    // accepted tasks or their results at a crash.
    let mut durable = with_redb_backends(&["local"]);
    let mut half_queue = with_redb_backends(&["local"]);
    let mut half_mailbox = with_redb_backends(&["local"]);
    let mut split = with_redb_backends(&["local", "other"]);
    for (cfg, queue, mailbox) in [
        (&mut durable, on("local"), on("local")),
        (&mut half_queue, on("local"), StorageConfig::InMemory),
        (&mut half_mailbox, StorageConfig::InMemory, on("local")),
        (&mut split, on("local"), on("other")),
    ] {
        if let Some(pool) = cfg.pools.get_mut("inference") {
            pool.queue = queue;
            pool.mailbox = mailbox;
        }
    }
    if !cfg!(feature = "embedded") {
        // Without the `embedded` feature the backend itself is rejected first.
        let err = durable.validate().expect_err("redb needs `embedded`");
        assert_eq!(err.field(), Some("backends.local.type"));
        return;
    }
    durable
        .validate()
        .expect("queue and mailbox on one backend");
    let field = |cfg: &SchedulerConfig| {
        let err = cfg.validate().expect_err("invalid storage");
        (err.field().map(str::to_owned), err.to_string())
    };
    let (path, message) = field(&half_queue);
    assert_eq!(
        path.as_deref(),
        Some("pools.inference.mailbox"),
        "{message}"
    );
    assert!(message.contains("loses results"), "{message}");
    let (path, message) = field(&half_mailbox);
    assert_eq!(path.as_deref(), Some("pools.inference.queue"), "{message}");
    assert!(message.contains("loses accepted tasks"), "{message}");
    let (path, message) = field(&split);
    assert_eq!(
        path.as_deref(),
        Some("pools.inference.mailbox"),
        "{message}"
    );
    assert!(message.contains("same backend"), "{message}");
}

#[test]
fn config_durable_backend_drives_at_most_one_pool() {
    let mut cfg = with_redb_backends(&["local"]);
    let mut second = pool(2);
    second.queue = on("local");
    second.mailbox = on("local");
    cfg.pools.insert("second".to_owned(), second.clone());
    cfg.pools.insert("third".to_owned(), second);
    let err = cfg.validate().expect_err("two pools on one backend");
    if cfg!(feature = "embedded") {
        assert_eq!(err.field(), Some("pools.third.queue"), "{err}");
        assert!(
            err.to_string().contains("already drives pool `second`"),
            "{err}"
        );
    } else {
        assert_eq!(err.field(), Some("backends.local.type"));
    }
}

#[cfg(feature = "embedded")]
#[test]
fn config_backend_values_are_validated() {
    let json = format!(
        r#"{{ "backends": {{ "local": {{ "type": "redb", "path": "./q.redb",
            "max_depth": 10, "lease_secs": 0 }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.local.lease_secs");

    let json = format!(
        r#"{{ "backends": {{ "local": {{ "type": "redb", "path": "./q.redb",
            "max_depth": 0, "lease_secs": 5 }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.local.max_depth");
}

#[cfg(feature = "embedded")]
#[test]
fn config_redb_max_attempts_zero_is_rejected() {
    let json = format!(
        r#"{{ "backends": {{ "local": {{ "type": "redb", "path": "./q.redb",
            "max_depth": 10, "lease_secs": 5, "max_attempts": 0 }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.local.max_attempts");
}

#[cfg(feature = "postgres")]
#[test]
fn config_postgres_values_are_validated_and_debug_hides_the_password() {
    let json = format!(
        r#"{{ "backends": {{ "db": {{ "type": "postgres", "url": "postgres://app:s3cret@db/app",
            "queue_name": "q", "max_depth": 10, "lease_secs": 30, "max_attempts": 0 }} }},
            "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.db.max_attempts");

    let parsed: SchedulerConfig = serde_json::from_str(&json).expect("parses");
    let debug = format!("{parsed:?}");
    assert!(
        !debug.contains("s3cret"),
        "Debug leaks the password: {debug}"
    );
    assert!(debug.contains("postgres://app:***@db/app"), "{debug}");
}

const PGMQ_BACKEND: &str = r#""type": "pgmq", "url": "postgres://app:s3cret@db/app",
                             "queue_name": "events", "max_depth": 10, "lease_secs": 30"#;

#[test]
fn config_misspelled_pgmq_backend_field_is_rejected_naming_it() {
    let json = format!(
        r#"{{ "backends": {{ "q": {{ {PGMQ_BACKEND}, "prepair": true }} }},
            "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let message = parse_error(&json);
    assert!(
        message.contains("unknown field `prepair`"),
        "error must name the misspelled field: {message}"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn config_pgmq_values_are_validated_and_debug_hides_the_password() {
    let ok = format!(
        r#"{{ "backends": {{ "q": {{ {PGMQ_BACKEND} }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let parsed = SchedulerConfig::from_json_str(&ok).expect("a valid pgmq section loads");
    let Some(BackendConfig::Pgmq(section)) = parsed.backends.get("q") else {
        panic!("`type: pgmq` parses as a pgmq section");
    };
    assert!(!section.listen && !section.prepare && section.max_attempts.is_none());
    let debug = format!("{parsed:?}");
    assert!(
        !debug.contains("s3cret"),
        "Debug leaks the password: {debug}"
    );
    assert!(debug.contains("postgres://app:***@db/app"), "{debug}");

    // The queue name becomes part of SQL identifiers: the rule
    // `PgmqLeaseQueue::new` enforces is checked at validation.
    let too_long = "q".repeat(39);
    for name in ["", "Events", "ev-ents", too_long.as_str()] {
        let json = ok.replace(
            r#""queue_name": "events""#,
            &format!(r#""queue_name": "{name}""#),
        );
        assert_eq!(
            invalid_field(&json),
            "backends.q.queue_name",
            "name {name:?}"
        );
    }
    let json = ok.replace(
        r#""lease_secs": 30"#,
        r#""lease_secs": 30, "max_attempts": 0"#,
    );
    assert_eq!(invalid_field(&json), "backends.q.max_attempts");
    let json = ok.replace(r#""max_depth": 10"#, r#""max_depth": 0"#);
    assert_eq!(invalid_field(&json), "backends.q.max_depth");
}

/// A pgmq backend is a queue for the application: no pool can run on it,
/// because pool-level crash durability on pgmq is not claimed (S-18).
#[cfg(feature = "postgres")]
#[test]
fn config_pool_on_a_pgmq_backend_is_rejected() {
    let durable = r#""max_units": 4, "max_queue_depth": 8, "default_timeout_secs": 30,
                     "queue": { "backend": "q" }, "mailbox": { "backend": "q" }"#;
    let json = format!(
        r#"{{ "backends": {{ "q": {{ {PGMQ_BACKEND} }} }}, "pools": {{ "p": {{ {durable} }} }} }}"#
    );
    let err = SchedulerConfig::from_json_str(&json).expect_err("no pool runs on pgmq");
    assert_eq!(err.field(), Some("pools.p.queue"));
    let message = err.to_string();
    assert!(
        message.contains("`pgmq`") && message.contains("postgres"),
        "the error names the backend type and the alternative: {message}"
    );

    let half = r#""max_units": 4, "max_queue_depth": 8, "default_timeout_secs": 30,
                  "queue": "in_memory", "mailbox": { "backend": "q" }"#;
    let json = format!(
        r#"{{ "backends": {{ "q": {{ {PGMQ_BACKEND} }} }}, "pools": {{ "p": {{ {half} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "pools.p.mailbox");
}

const IGGY_BACKEND: &str = r#""type": "iggy", "connection_string": "iggy://app:s3cret@iggy:8090""#;

#[cfg(feature = "iggy")]
#[test]
fn config_iggy_section_defaults_validates_and_builds_options() {
    let json = format!(
        r#"{{ "backends": {{ "ev": {{ {IGGY_BACKEND} }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let parsed =
        SchedulerConfig::from_json_str(&json).expect("an iggy section with defaults loads");
    let Some(BackendConfig::Iggy(section)) = parsed.backends.get("ev") else {
        panic!("`type: iggy` parses as an iggy section");
    };
    assert_eq!(section.stream, "parking-lot");
    assert_eq!(section.mailbox_topic, "mailbox.ready");
    assert_eq!(section.stream_topic, "stream.chunks");
    assert!(
        !format!("{section:?}").contains("s3cret"),
        "Debug hides the password"
    );
    let options = section.options();
    assert_eq!(options.lifecycle_topic, "lifecycle");
    assert_eq!(options.queue_capacity, 1024);
    assert_eq!(options.retry.max_attempts, 5);
    assert_eq!(options.stream_topic, "stream.chunks");

    let json = json.replace(IGGY_BACKEND, &format!("{IGGY_BACKEND}, \"partitions\": 0"));
    assert_eq!(invalid_field(&json), "backends.ev.partitions");
    let json = format!(
        r#"{{ "backends": {{ "ev": {{ {IGGY_BACKEND}, "strem": "x" }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert!(
        parse_error(&json).contains("strem"),
        "a misspelled field is named"
    );
}

/// `stream_topic` (cr-33) is configurable, reaches `options()`, and must not be
/// empty.
#[cfg(feature = "iggy")]
#[test]
fn config_iggy_stream_topic_reaches_the_options() {
    let section = format!(r#"{IGGY_BACKEND}, "stream_topic": "llm.tokens""#);
    let json = format!(
        r#"{{ "backends": {{ "ev": {{ {section} }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    let parsed = SchedulerConfig::from_json_str(&json).expect("a stream_topic loads");
    let Some(BackendConfig::Iggy(section)) = parsed.backends.get("ev") else {
        panic!("`type: iggy` parses as an iggy section");
    };
    assert_eq!(section.stream_topic, "llm.tokens");
    assert_eq!(section.options().stream_topic, "llm.tokens");

    let json = json.replace("llm.tokens", " ");
    assert_eq!(invalid_field(&json), "backends.ev.stream_topic");
}

/// An iggy backend carries events only: no pool can run on it.
#[cfg(feature = "iggy")]
#[test]
fn config_pool_on_an_iggy_backend_is_rejected() {
    let durable = r#""max_units": 4, "max_queue_depth": 8, "default_timeout_secs": 30,
                     "queue": { "backend": "ev" }, "mailbox": { "backend": "ev" }"#;
    let json = format!(
        r#"{{ "backends": {{ "ev": {{ {IGGY_BACKEND} }} }}, "pools": {{ "p": {{ {durable} }} }} }}"#
    );
    let err = SchedulerConfig::from_json_str(&json).expect_err("no pool runs on iggy");
    assert_eq!(err.field(), Some("pools.p.queue"));
}

#[cfg(not(feature = "iggy"))]
#[test]
fn config_iggy_backend_without_the_feature_is_rejected() {
    let json = format!(
        r#"{{ "backends": {{ "ev": {{ {IGGY_BACKEND} }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.ev.type");
}

#[cfg(not(feature = "postgres"))]
#[test]
fn config_pgmq_backend_without_the_feature_is_rejected() {
    let json = format!(
        r#"{{ "backends": {{ "q": {{ {PGMQ_BACKEND} }} }}, "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.q.type");
}

#[cfg(not(feature = "postgres"))]
#[test]
fn config_postgres_backend_without_the_feature_is_rejected() {
    let json = format!(
        r#"{{ "backends": {{ "db": {{ "type": "postgres", "url": "postgres://u@h/d",
            "queue_name": "q", "max_depth": 10, "lease_secs": 30 }} }},
            "pools": {{ "p": {{ {POOL} }} }} }}"#
    );
    assert_eq!(invalid_field(&json), "backends.db.type");
}

// ── documented examples ─────────────────────────────────────────────────────

/// The documented examples use every backend, so they need every feature.
#[cfg(all(feature = "postgres", feature = "embedded", feature = "iggy"))]
mod examples {
    use std::path::{Path, PathBuf};

    use prometheus_parking_lot::config::{BackendConfig, SchedulerConfig};

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// `examples/configs/*.json`, sorted.
    fn example_config_files() -> Vec<PathBuf> {
        let dir = repo_root().join("examples/configs");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .map(|entry| entry.expect("dir entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn config_every_example_file_loads_and_validates() {
        let files = example_config_files();
        let names: Vec<_> = files
            .iter()
            .map(|p| {
                p.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "iggy.json",
                "in-memory.json",
                "pgmq.json",
                "postgres.json",
                "redb.json"
            ]
        );
        for file in &files {
            let text = std::fs::read_to_string(file).expect("read example");
            if let Err(err) = SchedulerConfig::from_json_str(&text) {
                panic!("{} does not load: {err}", file.display());
            }
        }
    }

    /// Fenced `json` code blocks in `text`. For Rust sources, only doc comments
    /// (`//!`, `///`) are read, with their markers stripped.
    fn json_blocks(path: &Path, text: &str) -> Vec<String> {
        let is_rust = path.extension().is_some_and(|ext| ext == "rs");
        let lines = text.lines().filter_map(|line| {
            if !is_rust {
                return Some(line);
            }
            let trimmed = line.trim_start();
            trimmed
                .strip_prefix("//!")
                .or_else(|| trimmed.strip_prefix("///"))
                .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
        });
        let mut blocks = Vec::new();
        let mut current: Option<String> = None;
        for line in lines {
            let fence = line.trim_start();
            match current.as_mut() {
                None if fence.starts_with("```json") => current = Some(String::new()),
                Some(_) if fence.starts_with("```") => blocks.extend(current.take()),
                Some(block) => {
                    block.push_str(line);
                    block.push('\n');
                }
                None => {}
            }
        }
        blocks
    }

    fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                markdown_files(&path, out);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "md" || ext == "mdx")
            {
                out.push(path);
            }
        }
    }

    /// Every place a configuration example is documented: the README, the docs
    /// (not the archive or the audit), the docs site, and the config rustdoc.
    fn documentation_files() -> Vec<PathBuf> {
        let root = repo_root();
        let mut files = vec![root.join("README.md")];
        for dir in ["docs", "website/docs"] {
            let mut found = Vec::new();
            markdown_files(&root.join(dir), &mut found);
            files.extend(
                found
                    .into_iter()
                    .filter(|p| !p.components().any(|c| c.as_os_str() == "archive"))
                    .filter(|p| !p.ends_with("update-plan-10-22-2026.md")),
            );
        }
        for source in [
            "src/config/mod.rs",
            "src/config/pool.rs",
            "src/config/backend.rs",
        ] {
            files.push(root.join(source));
        }
        files.sort();
        files
    }

    #[test]
    fn config_every_json_example_in_the_docs_loads() {
        let mut checked = 0;
        for file in documentation_files() {
            let text = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
            for block in json_blocks(&file, &text) {
                if block.contains("\"pools\"") {
                    if let Err(err) = SchedulerConfig::from_json_str(&block) {
                        panic!(
                            "scheduler config in {} does not load: {err}\n{block}",
                            file.display()
                        );
                    }
                    checked += 1;
                } else if block.contains("\"type\"") {
                    if let Err(err) = serde_json::from_str::<BackendConfig>(&block) {
                        panic!(
                            "backend section in {} does not parse: {err}\n{block}",
                            file.display()
                        );
                    }
                    checked += 1;
                }
            }
        }
        // At least: pool.rs (one scheduler config), backend.rs (two backend
        // sections) and docs/ARCHITECTURE.md (one scheduler config).
        assert!(
            checked >= 4,
            "found only {checked} documented config examples"
        );
    }
}
