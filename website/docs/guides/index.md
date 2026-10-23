---
title: Guides
description: Scenario guides with compiled examples - GPU inference, a Tauri desktop app, a multi-tenant Postgres service, disconnected clients, shutdown, Iggy events and stream replay - plus configuration, upgrade notes and the API reference.
---

# Guides

Each scenario guide says when to use it, what it delivers and what it does not
do, and walks through an example program under `examples/`. Every Rust block
is an excerpt of that program, and CI checks that it still is; the programs are
built and run in CI. Every JSON configuration shown is loaded by the test
suite.

New here? [Choosing a scenario](./scenarios.md) maps your deployment to a guide.

## Scenarios

| Guide | Shows | Example | Needs |
| --- | --- | --- | --- |
| [GPU inference pool](./gpu-inference-pool.md) | Per-resource limits (VRAM and CPU slots), skip-ahead, never-fits rejection | `gpu_inference_pool` | nothing |
| [Tauri local LLM](./tauri-local-llm.md) | A desktop app's queue on a redb file that survives restarts | `bindings/tauri-desktop`, `shutdown_requeue` | nothing |
| [Cloud multi-tenant Postgres](./cloud-postgres.md) | Durable pools per tier, tenant-scoped results, safe client retries | `postgres_tenants` | PostgreSQL |
| [Disconnected clients](./disconnected-clients.md) | Mailbox addresses, consume-once, results across a service restart | `disconnected_client` | nothing |
| [Shutdown and requeue](./shutdown-and-requeue.md) | Shutdown policies, the report, requeue and restart | `shutdown_requeue` | nothing |
| [Iggy events](./iggy-events.md) | Lifecycle events and `mailbox.ready` on Apache Iggy | `iggy_events` | Iggy |
| [Streaming replay](./streaming-replay.md) | Token streams that resume with no gap and no repeat | `iggy_stream_replay` | Iggy |

## Reference

- [Configuration](./configuration.md): every pool and backend field, and what
  validation enforces.
- [Release and upgrade notes](./upgrading.md): moving from 0.2 to 0.3.0.
- [API reference](./api-reference.md): rustdoc for each crate, and the list of
  compiled examples.
