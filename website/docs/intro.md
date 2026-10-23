---
title: Overview
description: What Prometheus Parking Lot is, what this documentation covers, and how it is being built.
slug: /intro
---

# Prometheus Parking Lot

Prometheus Parking Lot is a Rust library that schedules resource-constrained
work. Each task declares what it costs; a pool runs the task when its cost fits
in the remaining capacity and otherwise parks it until capacity frees. Results
go to a mailbox, so a client that disconnects can collect them later.

The library is being reworked in the **correctness-and-durability** phase. Its
goals are that weighted capacity is never exceeded, that accepted work is never
silently lost, and that results reach clients that disconnected. On `main`,
every pool enforces weighted capacity, and a pool configured on a Postgres or
redb backend (built as a `DurableResourcePool`) keeps accepted work and its
result across a crash. The worker pools and in-memory pools do not survive a
restart yet. The
[changelog](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/CHANGELOG.md)
records what has shipped.

## How this documentation is built

The site was delivered in three OpenSpec changes.

```mermaid
flowchart LR
  site["cr-45<br/>Site and brand"] --> concepts["cr-46<br/>Concepts"]
  concepts --> guides["cr-47<br/>Guides and API reference"]
```

| Section | Change | Status |
| --- | --- | --- |
| Site, theme, search and verification | `cr-45-docs-site` | Merged |
| [Concepts](./concepts/index.md) | `cr-46-docs-theory` | Merged |
| [Guides](./guides/index.md) | `cr-47-docs-guides` | Written |

## Concepts

The concept chapters explain the ideas the library is built on, with primary
sources, and say which parts exist on `main` and which are planned:

- [Parking lots](./concepts/parking-lots.md): parking work instead of threads.
- [Admission control](./concepts/admission-control.md): weighted admission,
  bin packing, priority and starvation.
- [Leases and delivery](./concepts/leases.md): leases, at-least-once delivery
  and idempotency.
- [Mailboxes and actors](./concepts/mailboxes-and-actors.md): the result
  mailbox for disconnected clients.
- [Worker pools](./concepts/worker-pools.md): dedicated threads for heavy work.
- [Event logs](./concepts/event-logs.md): events and notifications.
- [Agentic value](./concepts/agentic-value.md): why agent workloads need all
  of the above together.

## Guides

The [guides](./guides/index.md) show the library working, one deployment per
page, each backed by an example program that CI builds and runs:
[GPU inference pool](./guides/gpu-inference-pool.md),
[Tauri local LLM](./guides/tauri-local-llm.md),
[Cloud multi-tenant Postgres](./guides/cloud-postgres.md),
[Disconnected clients](./guides/disconnected-clients.md),
[Shutdown and requeue](./guides/shutdown-and-requeue.md),
[Iggy events](./guides/iggy-events.md) and
[Streaming replay](./guides/streaming-replay.md). Reference pages cover
[configuration](./guides/configuration.md),
[upgrading from 0.2](./guides/upgrading.md) and the
[API reference](./guides/api-reference.md).

To build or check the site itself, see [Working on this site](./contributing/docs-site.md).
