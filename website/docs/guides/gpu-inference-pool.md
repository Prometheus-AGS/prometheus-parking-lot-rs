---
title: GPU inference pool
description: Admit LLM requests by GPU memory and CPU slots at once, reject requests that can never fit, and let small requests start ahead of a large one without starving it.
---

# GPU inference pool

**Use it when** several model requests share one GPU and the scarce thing is
memory, not request count: two large requests can need more VRAM than ten small
ones.

**What you get:** each request declares the VRAM and CPU slots it needs; the
pool starts it only when every one of those fits, rejects at submission a
request that never could, and lets a small request start ahead of a large one
that is waiting for memory, a bounded number of times.

Full source: [`examples/gpu_inference_pool.rs`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/gpu_inference_pool.rs).
It needs no server:

```bash
cargo run --example gpu_inference_pool
```

## The pool

One GPU with 24 units of VRAM (read a unit as 1 GiB) and four CPU slots for the
request handlers. `max_units` bounds the total over every kind;
`resource_limits` bounds each listed kind on its own; `max_head_skips` is the
starvation bound for skip-ahead (the default is 8).

```json
{
  "pools": {
    "inference": {
      "max_units": 40,
      "max_queue_depth": 64,
      "default_timeout_secs": 120,
      "queue": "in_memory",
      "mailbox": "in_memory",
      "max_head_skips": 8,
      "resource_limits": { "gpu_vram": 24, "cpu": 4 }
    }
  }
}
```

The builder turns the section into a `ResourcePool` with an in-memory queue and
mailbox. `default_timeout_secs` is the execution budget: a request still
running after 120 s is cancelled and its units are released.

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
    let inference = &built.pools["inference"];
    let pool = &inference.pool;
```

## A request declares what it needs

The cost is a vector: VRAM and one CPU slot.

```rust title="examples/gpu_inference_pool.rs"
            cost: ResourceCost::new(ResourceKind::GpuVram, vram).with(ResourceKind::Cpu, 1),
```

## What the pool decides

```rust title="examples/gpu_inference_pool.rs"
    // A 13B model at 16 GiB starts: 16 of 24 VRAM, 1 of 4 CPU slots.
    assert_eq!(
        pool.submit(request(1, "13b", 16)).await?,
        TaskStatus::Running
    );
    // A second 16 GiB request waits: 32 VRAM would exceed the 24 limit, even
    // though the pool's total (34 of 40) and the CPU slots have room.
    assert_eq!(
        pool.submit(request(2, "13b", 16)).await?,
        TaskStatus::Queued
    );
    // A 7B model at 6 GiB fits the VRAM that is left, so it starts ahead of
    // the parked 13B request. That is one skip of the head.
    let small = pool.submit(request(3, "7b", 6)).await?;
    println!("7b request: {small:?}");
    // A 30 GiB request can never fit 24 GiB of VRAM, although its total (31)
    // is within the pool's 40: rejected at submission, not parked.
    let err = pool.submit(request(4, "34b", 30)).await.unwrap_err();
    assert!(matches!(
        err,
        SchedulerError::CapacityExceeded {
            kind: Some(ResourceKind::GpuVram),
            ..
        }
    ));
    println!("34b request rejected: {err}");
```

The run prints:

```text
7b request: Running
34b request rejected: capacity exceeded: need 30 gpu_vram units, pool 'inference' has 24
running 2 | queued 1 | vram Some(22)/24 | cpu Some(2)/4 | skip-ahead SkipAheadStats { skipped: 1, reservations: 0 }
request 1: [13b] PROMPT 1
request 2: [13b] PROMPT 2
request 3: [7b] PROMPT 3
```

- **Per-kind admission.** The second 13B request waits although the pool's
  total has room (34 of 40 units) and so do the CPU slots: what stops it is the
  VRAM limit. Equally, a request waits when VRAM is free but all four CPU slots
  are taken.
- **Skip-ahead.** The 7B request starts ahead of the parked 13B one because it
  fits now. After a parked head has been passed `max_head_skips` times, the pool
  reserves the capacity for it and starts nothing else until it has started
  (`reservations` counts those). `0` keeps strict head-of-line order.
- **Never-fits is an error.** A request over any one limit returns
  `CapacityExceeded` at once instead of blocking the queue behind it. The error
  names the pool (its key in `pools`) and the kind that did not fit (`kind`;
  `None` when the total is what was exceeded).

Read per-kind usage with `pool.active_units_of(ResourceKind::GpuVram)` and the
skip counters with `pool.skip_ahead_stats()`.

## What this does not do

- **Units are what you declare.** The pool never measures the GPU. If a request
  under-declares its KV cache, the device can still run out of memory. Size the
  cost from the model's weights plus the longest context you allow.
- **One pool is one device.** Several GPUs need one pool each, and the routing
  between them is yours.
- **In memory only here.** This pool loses parked requests on a crash. For work
  that must survive a restart, put the pool on a durable backend
  ([Cloud Postgres](./cloud-postgres.md), [Tauri local LLM](./tauri-local-llm.md));
  a durable pool applies the same per-kind limits but dispatches in strict
  order, without skip-ahead.

Concepts: [Admission control](../concepts/admission-control.md).
