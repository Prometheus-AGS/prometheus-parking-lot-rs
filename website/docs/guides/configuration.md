---
title: Configuration
description: The scheduler configuration schema - pools, their limits and storage, and the postgres, redb, pgmq and iggy backends - with the rules validation enforces and what the builder makes of each.
---

# Configuration

A scheduler configuration is one JSON document with named `pools` and optional
named `backends`. `SchedulerConfig::from_json_str` parses and validates it;
`build_pools_from_scheduler_config` opens every backend and builds every pool.

Every JSON block on this site that contains `"pools"` is loaded with
`SchedulerConfig::from_json_str`, and every backend section (a block with a
`"type"`) is parsed as a `BackendConfig`, by the test
`config_every_json_example_in_the_docs_loads`; the files in
[`examples/configs/`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/tree/main/examples/configs)
are loaded by `config_every_example_file_loads_and_validates`. A block here
that stops matching the schema fails CI.

## The smallest configuration

```json
{
  "pools": {
    "llm_inference": {
      "max_units": 20,
      "max_queue_depth": 1000,
      "default_timeout_secs": 60,
      "queue": "in_memory",
      "mailbox": "in_memory"
    }
  }
}
```

## Pools

| Field | Required | Meaning |
| --- | --- | --- |
| `max_units` | yes | Capacity: the units of running tasks, summed over every resource kind, never exceed it. Greater than 0. |
| `max_queue_depth` | yes | Tasks that may wait. A submission beyond it fails with `QueueFull`. Greater than 0. (A durable pool is bounded by its backend's `max_depth` instead.) |
| `default_timeout_secs` | yes | Execution budget: a task still running after this long, or past its own deadline if that is sooner, is cancelled and its units released. Greater than 0. |
| `queue`, `mailbox` | yes | `"in_memory"`, or `{ "backend": "<name>" }`. Both the same: both in memory, or both naming one `postgres` or `redb` backend. |
| `max_head_skips` | no, default 8 | How often a parked task that does not fit may be passed by smaller ones that do before the pool reserves capacity for it. `0` is strict head-of-line order. |
| `resource_limits` | no | Units each kind may hold at once, keys `cpu`, `gpu_vram`, `io`, `mixed`. Each limit greater than 0 and at most `max_units`; a kind not listed is bounded by `max_units` alone. |

The schema is strict: an unknown or misspelled field fails to load, and the
error names it. A value that fails validation is reported with its dotted path,
for example `pools.inference.resource_limits.gpu_vram`.

What the builder makes of a pool:

- both `"in_memory"`: a `ResourcePool` with an in-memory queue and a
  `SharedInMemoryMailbox`, in `BuiltScheduler::pools`. Parked work and results
  are lost when the process exits.
- both `{ "backend": name }`: a started `DurableResourcePool` on that backend,
  in `BuiltScheduler::durable_pools`. Accepted tasks and their results survive
  a crash. It starts leasing at once, so work a previous process accepted and
  did not finish is picked up.

[GPU inference pool](./gpu-inference-pool.md) uses `resource_limits` and
`max_head_skips`. Building is one call:

```rust title="examples/gpu_inference_pool.rs"
    let config = SchedulerConfig::from_json_str(CONFIG)?;
    let gate = Arc::new(Semaphore::new(0));
    let built: BuiltScheduler<Generate, String, FakeModel, TokioSpawner> =
        build_pools_from_scheduler_config(
            &config,
            |_name, _pool| {
                Ok(FakeModel {
                    gate: Arc::clone(&gate),
                })
            },
            TokioSpawner::new(tokio::runtime::Handle::current()),
        )
        .await?;
```

The closure is called once per pool with its name and section, so one
executor type can serve pools with different models or tiers.
`build_pools_from_scheduler_config_with_hooks` takes a second closure that
returns the lifecycle hooks for each pool.

## Backends

Every backend section parses whatever features are enabled; validation rejects
one whose crate feature is off (`postgres` for `postgres` and `pgmq`, `embedded`
for `redb`, `iggy` for `iggy`).

| `"type"` | Feature | A pool can run on it | The builder opens |
| --- | --- | --- | --- |
| `postgres` | `postgres` | yes | a connection pool, the lease queue, the result mailbox, a `QueueListener` when `listen` is set; runs migrations when `migrate` is set |
| `redb` | `embedded` | yes | the redb file (created with its directories), its lease queue and result mailbox |
| `pgmq` | `postgres` | no | a `PgmqLeaseQueue` for your own consumers (`BuiltScheduler::pgmq`) |
| `iggy` | `iggy` | no | nothing: build `IggyHook` and `IggyNotifier` from `IggyBackendConfig::options()` |

Each `postgres` or `redb` backend drives at most one pool.

### `postgres`

```json
{ "type": "postgres", "url": "postgres://app@db/app", "queue_name": "inference",
  "max_depth": 10000, "lease_secs": 300, "max_attempts": 5, "listen": true, "migrate": true }
```

| Field | Required | Meaning |
| --- | --- | --- |
| `url` | yes | Connection URL. Inject the password from a secret store rather than writing it into the file; `Debug` output hides it. |
| `queue_name` | yes | Name of the queue and of its result mailbox. Queues with different names share tables, not entries. |
| `max_depth` | yes | Live entries (ready, delayed, leased) before `submit` fails with `QueueFull`. |
| `lease_secs` | yes | How long a leased task stays invisible before redelivery. A durable pool extends it every third of it, so it bounds how soon a crashed worker's tasks come back, not how long a task may run. |
| `max_attempts` | no | Dead-letter after this many leases. Absent: redeliver forever. |
| `listen` | no, `false` | Open a `LISTEN` connection (`QueueListener`). The durable pool itself polls. |
| `migrate` | no, `false` | Create the tables and indexes when missing (idempotent). |

[Cloud Postgres](./cloud-postgres.md) runs two of these.

### `redb`

```json
{ "type": "redb", "path": "./data/parking-lot.redb", "max_depth": 1000, "lease_secs": 300 }
```

`path`, `max_depth` and `lease_secs` are required. One process may hold the
file open at a time. [Tauri local LLM](./tauri-local-llm.md) and
[Shutdown and requeue](./shutdown-and-requeue.md) run on it.

### `pgmq`

```json
{ "type": "pgmq", "url": "postgres://app@db/app", "queue_name": "events",
  "max_depth": 10000, "lease_secs": 30, "max_attempts": 5, "listen": true, "prepare": true }
```

`queue_name` is 1 to 38 characters of `a-z`, `0-9` and `_`. `prepare` installs
the pgmq extension and creates the queue when missing. pgmq does not notify on
lease expiry, so consumers poll even with `listen`. Validation rejects a pool
whose `queue` or `mailbox` names a `pgmq` backend: pool-level crash durability
is claimed for `postgres` and `redb` only.

### `iggy`

```json
{ "type": "iggy", "connection_string": "iggy://app:***@iggy:8090", "stream": "parking-lot" }
```

Only `connection_string` is required (`***` stands for the password: inject
it from a secret store; `Debug` output hides it). The other fields and their defaults:
`stream` (`parking-lot`), `lifecycle_topic` (`lifecycle`), `mailbox_topic`
(`mailbox.ready`), `stream_topic` (`stream.chunks`), `partitions` (4),
`tenant` (absent: partition by pool), `queue_capacity` (1024),
`send_timeout_ms` (2000), `max_attempts` (5), `dedup_window` (4096). `IggyBackendConfig::options()` turns the section into
the `IggyOptions` that the hook, the notifier and the
[streaming replay](./streaming-replay.md) sink and reader take. See
[Iggy events](./iggy-events.md).

## A complete file

```json
{
  "backends": {
    "cloud": {
      "type": "postgres",
      "url": "postgres://parking_lot@localhost:5432/parking_lot",
      "queue_name": "inference",
      "max_depth": 10000,
      "lease_secs": 300,
      "max_attempts": 5,
      "listen": true,
      "migrate": true
    },
    "events": {
      "type": "iggy",
      "connection_string": "iggy://iggy:***@127.0.0.1:8090"
    }
  },
  "pools": {
    "llm_inference": {
      "max_units": 40,
      "max_queue_depth": 1000,
      "default_timeout_secs": 120,
      "queue": { "backend": "cloud" },
      "mailbox": { "backend": "cloud" },
      "resource_limits": { "gpu_vram": 24, "cpu": 4 }
    },
    "embeddings": {
      "max_units": 8,
      "max_queue_depth": 200,
      "default_timeout_secs": 45,
      "queue": "in_memory",
      "mailbox": "in_memory"
    }
  }
}
```

## `WorkerPool` configuration

`WorkerPoolConfig` is a separate struct with the same strictness, built in code
(`WorkerPoolConfig::new().with_worker_count(..)`, see the
[README](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs#example-workerpool))
or deserialized from JSON with serde. Its fields, all optional: `worker_count`
(default: the number of CPUs), `thread_stack_size` (2 MiB, ignored on WASM),
`max_units` (1000), `max_queue_depth` (1000), `default_timeout_ms` (120000: the
execution budget and, on native targets, the result TTL), `max_head_skips` (8)
and `resource_limits`.

## What the configuration cannot express yet

- A result TTL for a durable pool (results wait until consumed); use
  `DurableOptions::with_result_ttl` and start the pool yourself.
- A notifier on a durable pool's mailbox; see [Iggy events](./iggy-events.md).
- A durable pool's heartbeat and poll intervals (a third of the lease, and
  500 ms).
