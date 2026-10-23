---
title: Shutdown and requeue
description: Stop a pool on deploy or quit without silently losing accepted work - drain, cancel what is left, capture or requeue it, and account for every task in a report.
---

# Shutdown and requeue

**Use it when** the process stops on purpose: a deploy, a scale-down, a
desktop app quitting, a Ctrl-C. Accepted work must either finish, be handed
back, or be accounted for.

**What you get:** `shutdown_with(policy)` refuses new work at once, lets
accepted work run until the policy's drain deadline, then cancels what is left
and returns a `ShutdownReport` in which every accepted task either completed,
was cancelled, or is still running (`unsettled`). A durable pool hands each
cancelled task back to its queue, and the next pool on that queue runs it.

Full source: [`examples/shutdown_requeue.rs`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/shutdown_requeue.rs).
It uses the embedded redb backend and needs no server:

```bash
cargo run --example shutdown_requeue --features embedded
```

## Policies

| Policy | Waits for accepted work | Then |
| --- | --- | --- |
| `DrainThenCancel { deadline }` (default, 30 s) | until `deadline` | cancels what is still queued or running |
| `Drain { deadline }` | until `deadline` | cancels what is still queued; running tasks keep running and are counted `unsettled` |
| `CancelImmediately` | no | cancels everything |

## Durable pool: cancel, requeue, restart

Process 1 accepts three one-minute tasks on a pool that runs one at a time,
then shuts down with a 100 ms deadline:

```rust title="examples/shutdown_requeue.rs"
    {
        let scheduler = start(&path, Duration::from_secs(60)).await?;
        let jobs = &scheduler.durable_pools["jobs"];
        for id in 1..=3 {
            jobs.submit(task(id)).await?;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let report = jobs
            .shutdown_with(ShutdownPolicy::DrainThenCancel {
                deadline: Duration::from_millis(100),
            })
            .await;
        println!("process 1 shutdown: {report:?}");
        assert_eq!(report.completed, 0);
        assert_eq!(report.unsettled, 0, "nothing is left running");
        // Every task this process held when the deadline passed was cancelled
        // and handed back to the queue (`requeued`); tasks it had not leased
        // yet never left the queue.
        assert!(report.cancelled >= 1);
        assert_eq!(report.cancelled, report.requeued);
        assert!(jobs.consume(&inbox()).await?.is_none(), "no result yet");
    }
```

Process 2 opens the same file with a fast executor; all three tasks run, each
result once:

```text
process 1 shutdown: ShutdownReport { completed: 0, cancelled: 1, recorded: 0, requeued: 1, dropped: 0, unanswered: 0, unsettled: 0 }
process 2 results: ["done: job 1", "done: job 2", "done: job 3"]
```

The report counts the tasks process 1 held when the deadline passed. Tasks it
had not taken from the queue stay there and need no hand-back, so they do not
appear in it. CI runs the same shutdown and restart on redb and on PostgreSQL 17
and 18 (the `shutdown_durable_` tests).

## In-memory pools: the cancellation sink

An in-memory `ResourcePool` or `WorkerPool` has no queue to hand work back to.
Each cancelled task's mailbox (or result slot) receives `TaskError::Cancelled`,
and the task itself, with its payload when the pool has it, goes to the pool's
`CancellationSink`: by default one that logs it, or your own that writes it
somewhere durable for replay. The
[README's shutdown example](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs#shutdown-without-losing-work),
compiled and run as a doctest, uses `RecordingCancellationSink` to show both
the running and the queued task arriving there.

The report's `cancelled` is the sum of `recorded` (taken by the sink),
`requeued` (durable pool), `dropped` (the sink declined it) and `unanswered`
(the sink did not answer within `CancellationConfig::delta`).

## Wiring it to a signal

`wait_for_signal()` resolves on Ctrl-C (and SIGTERM on Unix). Await it, then
call `shutdown_with` on every pool; size the deadline below your orchestrator's
kill timeout (Kubernetes sends SIGKILL 30 s after SIGTERM by default), or the
process is killed mid-drain. A durable pool survives that too, through lease
expiry, but an in-memory pool loses what had not reached the sink.

## What this does not do

- **A cancelled task ran partway.** Cancellation happens at the executor's next
  `await`. Whatever it did before that (a file written, a call made) is not
  undone, and a requeued task runs again from the start.
- **The native `WorkerPool` cannot return a running task's payload.** Its sink
  receives the task with `payload: None`; tasks that never started carry it.
- **`unsettled` is not lost, but it is not finished either.** Under `Drain`, or
  with an executor that blocks without yielding, `shutdown_with` returns while
  tasks still run; they deliver later if the process lives that long.

Concepts: [Worker pools](../concepts/worker-pools.md),
[Leases and delivery](../concepts/leases.md).
