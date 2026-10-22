//! Crash-safety tests for the redb queue and mailbox (scenario S-8).
//!
//! # How a crash is injected
//!
//! Each `crash_point_*` test runs this test binary again as a child process
//! (`crash_child_entry`, selected by the `PLQ_CHILD_DB` environment variable).
//! The child runs a fixed workload through the real `EmbeddedStore`, queue and
//! mailbox. With the `fault-injection` feature the library stops the child at
//! one named point inside one operation (`<operation>:before_commit` while the
//! write transaction is open and uncommitted, `<operation>:after_commit` right
//! after the durable commit, before the caller hears about it): the child prints
//! `CRASHPOINT <point>` and parks. The parent then sends **SIGKILL**
//! (`Child::kill`), so no destructor, flush or unwinding runs, and nothing in
//! the child gets a chance to tidy the database.
//!
//! The parent reopens the file in-process, with a clock an hour ahead so every
//! lease has expired, and checks the invariants in [`verify`] against the log
//! of operations the child had completed.
//!
//! This models a process crash faithfully: the killed process's file writes
//! are in the OS page cache and survive, exactly as after a real SIGKILL or
//! panic-abort. It does not model power loss (where unsynced pages vanish);
//! that is redb's fsync-on-commit contract (`Durability::Immediate`, the
//! default this crate keeps), which these tests do not re-prove.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use prometheus_parking_lot_core::{
    Clock, Delivery, DeliveryOutcome, LeaseQueue, MailboxKey, NackAction, Priority, ResourceCost,
    ResourceKind, ResultMailbox, ScheduledTask, TaskMetadata, TaskStatus, TestClock, now_ms,
};
use prometheus_parking_lot_embedded::EmbeddedStore;

const CHILD_ENV: &str = "PLQ_CHILD_DB";
const FAULT_ENV: &str = "PARKING_LOT_FAULT_POINT";
const PAUSE_ENV: &str = "PLQ_CHILD_PAUSE_AFTER";
const LONG_LEASE: Duration = Duration::from_secs(600);

fn mailbox_key() -> MailboxKey {
    MailboxKey {
        tenant: "crash".to_owned(),
        user_id: Some("user".to_owned()),
        session_id: None,
    }
}

/// A task whose payload, task id source and idempotency key are `name`.
fn task(name: &str, priority: Priority) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id: 0,
            mailbox: Some(mailbox_key()),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: Some(name.to_owned()),
            sequence: 0,
        },
        payload: name.to_owned(),
    }
}

fn result_of(name: &str) -> String {
    format!("result-of-{name}")
}

/// Record a completed or starting step. When the parent asked the child to stop
/// after this step (`PLQ_CHILD_PAUSE_AFTER`), park here so it can be killed at
/// exactly this boundary between two operations.
fn log(line: &str) {
    use std::io::Write;
    println!("LOG {line}");
    if std::env::var(PAUSE_ENV).is_ok_and(|after| after == line) {
        println!("CRASHPOINT between:{line}");
        let _ = std::io::stdout().flush();
        loop {
            std::thread::park();
        }
    }
}

// ── The child ───────────────────────────────────────────────────────────────

/// Entry point of the child process; a no-op in a normal test run.
#[tokio::test]
async fn crash_child_entry() {
    let Ok(path) = std::env::var(CHILD_ENV) else {
        return;
    };
    let store = EmbeddedStore::open(&path).expect("child: open store");
    let queue = store.lease_queue::<String>(100);
    let mailbox = store.result_mailbox::<String>();
    log("OPEN");

    for (name, priority) in [("a", Priority::Low), ("b", Priority::High)] {
        log(&format!("ENQUEUING {name}"));
        let _ = queue.enqueue(task(name, priority)).await.expect("enqueue");
        log(&format!("ENQUEUED {name}"));
    }

    // The high-priority task is leased first; run it to completion.
    log("LEASING");
    let Some(first) = queue.lease(1, LONG_LEASE).await.expect("lease").pop() else {
        log("LEASED nothing");
        log("DONE");
        return;
    };
    let name = first.task.payload.clone();
    log(&format!("LEASED {name} {}", first.attempt));
    log(&format!("DELIVERING {name}"));
    let delivery = Delivery::new(
        mailbox_key(),
        1,
        TaskStatus::Completed,
        Some(result_of(&name)),
    )
    .with_idempotency_key(name.clone());
    let _ = mailbox.deliver(delivery).await.expect("deliver");
    log(&format!("DELIVERED {name}"));
    log(&format!("ACKING {name}"));
    queue.ack(first.token).await.expect("ack");
    log(&format!("ACKED {name}"));
    log("CONSUMING");
    let consumed = mailbox.consume(&mailbox_key()).await.expect("consume");
    log(&format!("CONSUMED {}", consumed.is_some()));

    // The low-priority task: lease, heartbeat, give back, lease again, and
    // leave the last lease held when the process ends.
    log("LEASING");
    let Some(second) = queue.lease(1, LONG_LEASE).await.expect("lease").pop() else {
        log("LEASED nothing");
        log("DONE");
        return;
    };
    let name = second.task.payload.clone();
    log(&format!("LEASED {name} {}", second.attempt));
    log(&format!("EXTENDING {name}"));
    queue
        .extend(second.token, LONG_LEASE)
        .await
        .expect("extend");
    log(&format!("EXTENDED {name}"));
    log(&format!("NACKING {name}"));
    queue
        .nack(second.token, NackAction::requeue())
        .await
        .expect("nack");
    log(&format!("NACKED {name}"));
    log("LEASING");
    if let Some(third) = queue.lease(1, LONG_LEASE).await.expect("lease").pop() {
        log(&format!("LEASED {} {}", third.task.payload, third.attempt));
    }
    log("DONE");
}

// ── The parent ──────────────────────────────────────────────────────────────

struct ChildRun {
    /// The `LOG` lines the child emitted before it stopped.
    log: Vec<String>,
    /// The crash point the child reported reaching, if any.
    reached: Option<String>,
    /// True when the child ran to the end and exited successfully.
    completed: bool,
}

/// Run the workload in a child process, `SIGKILL`ing it at `point` (a fault
/// point inside an operation) or after the step logged as `pause_after` (a
/// boundary between two operations).
fn run_child(db: &Path, point: Option<&str>, pause_after: Option<&str>) -> ChildRun {
    let mut command = Command::new(std::env::current_exe().expect("current exe"));
    command
        .args([
            "--exact",
            "crash_child_entry",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, db)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(point) = point {
        command.env(FAULT_ENV, point);
    } else {
        command.env_remove(FAULT_ENV);
    }
    if let Some(step) = pause_after {
        command.env(PAUSE_ENV, step);
    } else {
        command.env_remove(PAUSE_ENV);
    }
    let mut child = command.spawn().expect("spawn child");
    let stdout = child.stdout.take().expect("child stdout");

    let mut run = ChildRun {
        log: Vec::new(),
        reached: None,
        completed: false,
    };
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("read child stdout");
        if let Some(entry) = line.strip_prefix("LOG ") {
            run.log.push(entry.to_owned());
        } else if let Some(reached) = line.strip_prefix("CRASHPOINT ") {
            run.reached = Some(reached.to_owned());
            child.kill().expect("SIGKILL the child");
            break;
        }
    }
    let status = child.wait().expect("wait for child");
    run.completed = status.success();
    run
}

/// What the child's log says it did.
#[derive(Default)]
struct Journal {
    enqueue_started: BTreeSet<String>,
    enqueued: BTreeSet<String>,
    deliver_started: BTreeSet<String>,
    delivered: BTreeSet<String>,
    ack_started: BTreeSet<String>,
    acked: BTreeSet<String>,
    consume_started: bool,
    consumed: bool,
    /// Highest attempt number the child saw for each task.
    leased_attempt: BTreeMap<String, u32>,
}

impl Journal {
    fn parse(lines: &[String]) -> Self {
        let mut journal = Self::default();
        for line in lines {
            let mut words = line.split_whitespace();
            let (Some(verb), arg) = (words.next(), words.next()) else {
                continue;
            };
            let name = arg.unwrap_or_default().to_owned();
            match verb {
                "ENQUEUING" => {
                    journal.enqueue_started.insert(name);
                }
                "ENQUEUED" => {
                    journal.enqueued.insert(name);
                }
                "DELIVERING" => {
                    journal.deliver_started.insert(name);
                }
                "DELIVERED" => {
                    journal.delivered.insert(name);
                }
                "ACKING" => {
                    journal.ack_started.insert(name);
                }
                "ACKED" => {
                    journal.acked.insert(name);
                }
                "CONSUMING" => journal.consume_started = true,
                "CONSUMED" => journal.consumed = arg == Some("true"),
                "LEASED" => {
                    let attempt: u32 = words.next().and_then(|a| a.parse().ok()).unwrap_or(0);
                    let seen = journal.leased_attempt.entry(name).or_default();
                    *seen = (*seen).max(attempt);
                }
                _ => {}
            }
        }
        journal
    }
}

/// Reopen the database the killed child left behind and check that no accepted
/// work was lost, nothing acknowledged came back, and every stored result is
/// consumable exactly once.
async fn verify(db: &Path, journal: &Journal, context: &str) {
    // An hour ahead: every lease the child took has expired.
    let clock = TestClock::new(now_ms() + 3_600_000);
    assert!(clock.now_ms() > 0);
    let store = EmbeddedStore::open_with_clock(db, clock)
        .unwrap_or_else(|e| panic!("{context}: reopen after crash failed: {e}"));
    let queue = store.lease_queue::<String>(100);
    let mailbox = store.result_mailbox::<String>();

    // ── queue ──
    let live = queue
        .lease(100, Duration::from_secs(60))
        .await
        .unwrap_or_else(|e| panic!("{context}: lease after crash: {e}"));
    let mut seen = BTreeMap::new();
    for leased in &live {
        let name = leased.task.payload.clone();
        assert!(
            journal.enqueue_started.contains(&name),
            "{context}: task {name} appeared although its enqueue never started"
        );
        assert!(
            seen.insert(name.clone(), leased.attempt).is_none(),
            "{context}: task {name} is in the queue more than once"
        );
    }
    for name in &journal.enqueued {
        assert!(
            seen.contains_key(name) || journal.ack_started.contains(name),
            "{context}: ACCEPTED TASK LOST: {name} was enqueued, never acked, yet is gone (live: {:?})",
            seen.keys().collect::<Vec<_>>()
        );
    }
    for name in &journal.acked {
        assert!(
            !seen.contains_key(name),
            "{context}: ACKED TASK RETURNED: {name} was acked yet is back in the queue"
        );
    }
    for (name, attempt) in &seen {
        if let Some(before) = journal.leased_attempt.get(name) {
            assert!(
                attempt > before,
                "{context}: {name} was leased at attempt {before} and redelivered at attempt {attempt}"
            );
        }
    }
    let depth = queue.depth().await.expect("depth");
    assert_eq!(
        depth.leased,
        live.len(),
        "{context}: depth disagrees with leases"
    );
    assert_eq!(
        depth.ready, 0,
        "{context}: leased everything, nothing is ready"
    );

    verify_mailbox(&mailbox, journal, context).await;
}

/// The mailbox half of [`verify`]: stored results are consumable exactly once
/// and a repeated delivery keeps the first write.
async fn verify_mailbox(
    mailbox: &prometheus_parking_lot_embedded::RedbResultMailbox<String>,
    journal: &Journal,
    context: &str,
) {
    let mut results = Vec::new();
    while let Some(entry) = mailbox.consume(&mailbox_key()).await.expect("consume") {
        results.push(entry);
    }
    let mut names_with_result = BTreeSet::new();
    for entry in &results {
        let name = entry
            .idempotency_key
            .clone()
            .expect("results carry the idempotency key");
        assert!(
            names_with_result.insert(name.clone()),
            "{context}: result for {name} delivered more than once"
        );
        assert_eq!(entry.payload.as_deref(), Some(result_of(&name).as_str()));
    }
    for name in &journal.delivered {
        assert!(
            names_with_result.contains(name) || journal.consume_started,
            "{context}: STORED RESULT LOST: result for {name} was delivered, never consumed, yet is gone"
        );
        if journal.consumed {
            assert!(
                !names_with_result.contains(name),
                "{context}: CONSUMED RESULT RETURNED for {name}"
            );
        }
    }
    // A redelivered task re-delivers its result: the first write must stand.
    for name in &journal.deliver_started {
        let existed = names_with_result.contains(name) || journal.delivered.contains(name);
        let again = Delivery::new(
            mailbox_key(),
            1,
            TaskStatus::Completed,
            Some("second".to_owned()),
        )
        .with_idempotency_key(name.clone());
        let outcome = mailbox.deliver(again).await.expect("redeliver");
        if existed {
            assert_eq!(
                outcome,
                DeliveryOutcome::Duplicate,
                "{context}: a second result for {name} was stored"
            );
        }
    }
}

async fn crash_at(point: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("crash.redb");
    let run = run_child(&db, Some(point), None);
    assert_eq!(
        run.reached.as_deref(),
        Some(point),
        "the child never reached crash point {point} (log: {:?})",
        run.log
    );
    assert!(!run.completed, "a killed child cannot report success");
    verify(&db, &Journal::parse(&run.log), point).await;
}

/// Kill the child right after the step it logged as `step` returned.
async fn crash_between(step: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("between.redb");
    let run = run_child(&db, None, Some(step));
    let expected = format!("between:{step}");
    assert_eq!(
        run.reached.as_deref(),
        Some(expected.as_str()),
        "the child never completed step {step:?} (log: {:?})",
        run.log
    );
    assert!(!run.completed, "a killed child cannot report success");
    verify(&db, &Journal::parse(&run.log), &expected).await;
}

macro_rules! crash_point_tests {
    ($($name:ident => $point:literal),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                crash_at($point).await;
            }
        )*
    };
}

crash_point_tests! {
    crash_point_enqueue_before_commit => "enqueue:before_commit",
    crash_point_enqueue_after_commit => "enqueue:after_commit",
    crash_point_lease_before_commit => "lease:before_commit",
    crash_point_lease_after_commit => "lease:after_commit",
    crash_point_deliver_before_commit => "deliver:before_commit",
    crash_point_deliver_after_commit => "deliver:after_commit",
    crash_point_ack_before_commit => "ack:before_commit",
    crash_point_ack_after_commit => "ack:after_commit",
    crash_point_consume_before_commit => "consume:before_commit",
    crash_point_consume_after_commit => "consume:after_commit",
    crash_point_extend_before_commit => "extend:before_commit",
    crash_point_extend_after_commit => "extend:after_commit",
    crash_point_nack_before_commit => "nack:before_commit",
    crash_point_nack_after_commit => "nack:after_commit",
}

macro_rules! crash_between_tests {
    ($($name:ident => $step:literal),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                crash_between($step).await;
            }
        )*
    };
}

// Between each pair of steps of the workload: enqueue, lease, deliver, ack,
// consume, extend, nack, lease.
crash_between_tests! {
    crash_point_between_enqueue_a_and_enqueue_b => "ENQUEUED a",
    crash_point_between_enqueue_and_lease => "ENQUEUED b",
    crash_point_between_lease_and_deliver => "LEASED b 1",
    crash_point_between_deliver_and_ack => "DELIVERED b",
    crash_point_between_ack_and_consume => "ACKED b",
    crash_point_between_consume_and_lease => "CONSUMED true",
    crash_point_between_lease_and_extend => "LEASED a 1",
    crash_point_between_extend_and_nack => "EXTENDED a",
    crash_point_between_nack_and_lease => "NACKED a",
}

/// Control: with no crash point armed the workload completes, and a reopen
/// finds the single task the child left leased, redelivered.
#[tokio::test]
async fn crash_point_none_runs_to_completion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("control.redb");
    let run = run_child(&db, None, None);
    assert!(run.completed, "control run failed: {:?}", run.log);
    assert!(run.reached.is_none());
    assert_eq!(run.log.last().map(String::as_str), Some("DONE"));
    verify(&db, &Journal::parse(&run.log), "control").await;
}
