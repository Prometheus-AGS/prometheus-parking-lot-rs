---
title: Choosing a scenario
description: Which pool, backend and add-ons fit your deployment - a GPU server, a desktop app, a multi-tenant cloud service - and which guide shows it working.
---

# Choosing a scenario

Each guide below is one deployment shape, backed by an example program that CI
builds and runs. Start from the question that matches your situation.

## What runs the work?

| If the work is | Use | Because |
| --- | --- | --- |
| Async (HTTP calls to a model server, database work) and its result should wait for a client | `ResourcePool` | It runs on your tokio runtime and delivers each outcome to a mailbox address. |
| CPU- or GPU-bound in this process, or its result is not serializable (a channel, a tensor handle) | `WorkerPool` | Dedicated OS threads keep heavy work off your async runtime; the caller waits for the result with `retrieve`. |
| Expensive and must survive a crash or a deploy | `DurableResourcePool` (a pool section on a `postgres` or `redb` backend) | Accepted work is on disk before `submit` returns; results are exactly-once. |

## Where does it run?

| Deployment | Backend | Guide |
| --- | --- | --- |
| One GPU server, requests limited by VRAM | in memory, or a durable backend | [GPU inference pool](./gpu-inference-pool.md) |
| A desktop app running a local model | `redb` file | [Tauri local LLM](./tauri-local-llm.md) |
| A hosted multi-tenant service | `postgres` | [Cloud multi-tenant Postgres](./cloud-postgres.md) |
| Android or iOS | `redb` through the UniFFI bindings | [`bindings/README.md`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/bindings/README.md) |
| A browser or Node (`wasm32`) | in memory only | `WorkerPool` from the `prometheus-parking-lot-wasm` crate; see the [API reference](./api-reference.md) |

## What else do you need?

| Need | Guide |
| --- | --- |
| Clients that disconnect and come back for results | [Disconnected clients](./disconnected-clients.md) |
| Stop for a deploy without losing accepted work | [Shutdown and requeue](./shutdown-and-requeue.md) |
| Other services react to task events, or a gateway tells clients a result is ready | [Iggy events](./iggy-events.md) |
| Clients see tokens as they are generated and resume after a drop | [Streaming replay](./streaming-replay.md) |
| Everything above from one file | [Configuration](./configuration.md) |

Why these pieces exist together: [Agentic value](../concepts/agentic-value.md).
