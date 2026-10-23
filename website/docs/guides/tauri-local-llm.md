---
title: Tauri local LLM
description: Queue local model calls in a desktop app on an embedded redb file, so accepted prompts and their answers survive the app being closed, crashed or updated.
---

# Tauri local LLM

**Use it when** a desktop app runs a model on the user's machine and a prompt
can take longer than the user keeps the window open, or the app can be quit,
crash or update in the middle of a run.

**What you get:** a prompt is on disk the moment `submit` returns, and its
answer is on disk the moment it is stored. The next launch finds both. There is
no server to run: the queue and the mailbox are one redb file in the app's data
directory.

There are two ways to wire it, and the working Tauri 2 app uses the first.

## Option A: the store directly (the Tauri example)

[`bindings/tauri-desktop`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/tree/main/bindings/tauri-desktop)
is a complete Tauri 2 app. Its logic lives in the `queue-service` crate, which
calls the same `ParkingLotStore` the Android and iOS bindings use: `submit`
enqueues, `process_next` leases one task, runs it, stores the result and then
acks, and `collect` drains the mailbox. The shell opens the store once at
`app_data_dir()/parking-lot.redb`. Its restart test kills the store between
steps and checks nothing is lost:

```bash
cd bindings/tauri-desktop
cargo test --locked          # restart tests, no GUI
```

[`bindings/README.md`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/bindings/README.md)
covers the store's API, its lifecycle rules (one open store per file, open it
off the main thread) and where to put the file on each platform. This guide
does not repeat them.

Choose this when payloads are bytes and you want the one API that is also on
mobile. You write the worker loop, so you also decide how many prompts run at
once; nothing here enforces a memory limit.

## Option B: a durable pool in the Tauri backend

A Tauri backend is Rust in the same process as the window, so it can use the
facade crate with the `embedded` feature and get a `DurableResourcePool` on the
redb file. The pool adds what option A leaves to you: admission by declared
cost (one model loaded, so one prompt at a time, or a VRAM budget), an
execution budget, lease heartbeats for long generations, and lifecycle events.

The configuration names the file and puts the pool on it:

```json
{
  "backends": {
    "local": {
      "type": "redb",
      "path": "./data/parking-lot.redb",
      "max_depth": 1000,
      "lease_secs": 300
    }
  },
  "pools": {
    "tauri_local_llm": {
      "max_units": 8,
      "max_queue_depth": 200,
      "default_timeout_secs": 45,
      "queue": { "backend": "local" },
      "mailbox": { "backend": "local" }
    }
  }
}
```

In an app, build the path from `app.path().app_data_dir()` rather than a
relative one. The [shutdown example](./shutdown-and-requeue.md) builds exactly
this from code and restarts it on the same file:

```rust title="examples/shutdown_requeue.rs"
fn config(path: &Path) -> Result<SchedulerConfig, Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::json!({
        "backends": {
            "local": {
                "type": "redb",
                "path": path,
                "max_depth": 100,
                "lease_secs": 30
            }
        },
        "pools": {
            "jobs": {
                "max_units": 1,
                "max_queue_depth": 100,
                "default_timeout_secs": 60,
                "queue": { "backend": "local" },
                "mailbox": { "backend": "local" }
            }
        }
    });
    Ok(SchedulerConfig::from_json_str(&json.to_string())?)
}
```

Open it once in Tauri's `setup` with `build_pools_from_scheduler_config`, keep
the `BuiltScheduler` in managed state, and expose thin commands: `submit` calls
`durable_pools["tauri_local_llm"].submit`, `collect` calls `consume` with the
window's mailbox address. On quit, call
`shutdown_with(ShutdownPolicy::DrainThenCancel { .. })`: a generation still
running at the deadline goes back to the queue and runs on the next launch.
[Disconnected clients](./disconnected-clients.md) shows the submit, restart and
collect cycle end to end.

## What this does not do

- **One process per file.** redb locks the file; a second app instance (or a
  helper process) cannot open it at the same time. Route all access through the
  instance that holds it.
- **A re-run is a re-run.** Delivery is at-least-once: a generation interrupted
  by a crash runs again from the start after its lease expires. The idempotency
  key stops a second stored answer, not a second generation. Keep side effects
  of a prompt idempotent.
- **A prompt that crashes the app crashes it again.** A task that kills the
  process is redelivered on every launch until it is dead-lettered. Set
  `max_attempts` on the `redb` backend (for example 3) to give up on it after
  that many deliveries; it then waits in the dead letters
  (`RedbLeaseQueue::dead_letters`) instead of crashing every start.
- **Long leases delay recovery.** `lease_secs` is how long a crashed run blocks
  its prompt before the next launch picks it up; the pool's heartbeat keeps a
  live run's lease from expiring, so a short lease is safe.
- Option B is not run in the Tauri example or in CI as a Tauri app; the pool on
  redb is what `examples/shutdown_requeue.rs` and
  `examples/disconnected_client.rs` run.

Concepts: [Leases and delivery](../concepts/leases.md).
