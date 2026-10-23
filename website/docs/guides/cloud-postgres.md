---
title: Cloud multi-tenant Postgres
description: Run durable pools on PostgreSQL for a multi-tenant service, so accepted requests and their results survive a crash and a client retry is answered once.
---

# Cloud multi-tenant Postgres

**Use it when** a hosted service accepts expensive work from many tenants, the
process can be redeployed or killed at any time, and losing an accepted request
or answering it twice costs money.

**What you get:** `submit` returns only after the request is committed to a
Postgres lease queue. The pool holds a lease while the request waits for
capacity or runs, extends it with a heartbeat, stores the outcome in a Postgres
result mailbox under the request's idempotency key, and only then acks. A
process killed at any point has its requests re-run after the lease expires,
with exactly one result consumable. The tenant in each mailbox address keeps
tenants' results apart.

Full source: [`examples/postgres_tenants.rs`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/postgres_tenants.rs).
It needs PostgreSQL with the project image (PostgreSQL 18 plus pgmq; 17 works
too):

```bash
docker compose -f docker-compose.test.yml up -d --build postgres18
cargo run --example postgres_tenants --features postgres
```

## One backend per pool

A `"type": "postgres"` backend is a lease queue and a result mailbox in one
database. A pool whose `queue` and `mailbox` both name it runs as a
`DurableResourcePool` on it. Each backend drives at most one pool, so two
service tiers are two backends; they can share a database through different
`queue_name`s:

```json
{
  "backends": {
    "premium_db": {
      "type": "postgres",
      "url": "postgres://parking_lot@db.internal:5432/parking_lot",
      "queue_name": "premium",
      "max_depth": 10000,
      "lease_secs": 60,
      "max_attempts": 5,
      "listen": true,
      "migrate": true
    },
    "standard_db": {
      "type": "postgres",
      "url": "postgres://parking_lot@db.internal:5432/parking_lot",
      "queue_name": "standard",
      "max_depth": 10000,
      "lease_secs": 60,
      "max_attempts": 5,
      "listen": true,
      "migrate": true
    }
  },
  "pools": {
    "premium": {
      "max_units": 8,
      "max_queue_depth": 10000,
      "default_timeout_secs": 120,
      "queue": { "backend": "premium_db" },
      "mailbox": { "backend": "premium_db" }
    },
    "standard": {
      "max_units": 2,
      "max_queue_depth": 10000,
      "default_timeout_secs": 120,
      "queue": { "backend": "standard_db" },
      "mailbox": { "backend": "standard_db" }
    }
  }
}
```

- `migrate` creates the tables on start (idempotent); turn it off where
  migrations run separately.
- `listen` opens a `QueueListener` (`LISTEN/NOTIFY`) for the queue, returned in
  `BuiltScheduler::postgres[name].listener` for consumers of your own. The
  durable pool does not use it: it polls an empty queue every 500 ms, so a
  request submitted to an idle pool by another process can wait up to that long
  to start.
- `max_attempts` dead-letters a request after that many leases, so a request
  that crashes its worker every time stops being retried.
- `lease_secs` is how long a crashed worker blocks its request. The heartbeat
  extends a live lease every third of it.
- Keep the password out of the file: build the `url` from a secret at start-up,
  as the example does with `DATABASE_URL`. `Debug` output hides it.

## Several processes on one queue

Run the same configuration in as many processes as you like: every replica's
pool consumes the same `queue_name`, and a request is still answered exactly
once. The pools' names do not matter; replicas can share a name or each have
their own. A result delivered without a request idempotency key is keyed by
the queue, not the pool: `"<queue identity>:entry:<entry id>"`, where the
identity of a `PgLeaseQueue` is `pg:pl_lease_queue:<queue_name>`. So when a
replica dies after storing a result and before acking it, the replica that
picks the request up runs it again but stores nothing new; and pools on
different `queue_name`s can share one mailbox without dropping each other's
results.

Two limits. Execution is at-least-once: the request can run twice, so make
external side effects idempotent. And if you drop and recreate the queue table
(its id sequence restarts) while the mailbox table keeps old keys, a new
request can reuse an old entry id and its result would be taken for a
duplicate; recreate the mailbox with the queue, or give every request an
idempotency key. The same holds for two databases whose queues have the same
`queue_name` and deliver into one mailbox.

## Tenants share a pool, not results

The example builds the configuration from code and the URL from the
environment, then submits for two tenants:

```rust title="examples/postgres_tenants.rs"
fn request(id: u64, to: &MailboxKey, request_key: &str, prompt: &str) -> ScheduledTask<Completion> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(to.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            // The client's request id, scoped by tenant: a retry carries the same key.
            idempotency_key: Some(format!("{}:{request_key}", to.tenant)),
            sequence: 0,
        },
        payload: Completion {
            prompt: prompt.into(),
        },
    }
}
```

Scope the idempotency key by tenant, as here: two tenants may both send a
request `req-1`.

```rust title="examples/postgres_tenants.rs"
    // The client timed out and retried: same request key, same tenant.
    let retry = standard
        .submit(request(3, &globex, "req-1", "draft an email"))
        .await?;
    assert_eq!(retry, TaskStatus::Queued, "a duplicate counts as accepted");
```

```text
acme:   ["premium tier answer to 'plan the launch'"]
globex: ["standard tier answer to 'draft an email'"]
```

The retry resolves to the stored request; there is one answer, and the other
tenant's mailbox stays empty.

## Retries and duplicates, precisely

| When the retry arrives | What happens |
| --- | --- |
| The first request is still queued, leased or running | Nothing new is stored; `submit` returns `Queued` for the existing request. |
| The first request has finished | A new request is queued and the executor runs again, but the mailbox remembers the key and stores no second result. |
| The process crashed mid-run | The lease expires, the request runs again, and the first result stored wins. |

Results are exactly-once; execution is at-least-once. An executor that charges a
card or sends an email must make that side effect idempotent itself.

## Operating it

- [PostgreSQL 18 runbook](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/docs/runbooks/postgres18.md):
  data checksums, the image's volume path, and upgrading from 17.
- Crash tests: CI kills a pool with `SIGKILL` mid-execution and between
  execution and the mailbox write, on PostgreSQL 17 and 18, and checks every
  request has exactly one result.
- A `"type": "pgmq"` backend opens a lease queue on the pgmq extension for
  direct use. No pool runs on it: pool-level crash durability is only claimed
  for `"type": "postgres"`.

## What this does not do

- **No per-tenant fairness.** A pool admits by priority and submission order.
  One tenant that floods a pool delays every other tenant in it, up to
  `max_depth`. Separate tiers into pools (as above), or rate-limit tenants
  before `submit`.
- **One node per pool.** Several processes can lease from one queue, but each
  process's `max_units` is its own: there is no cluster-wide capacity limit.
- **Keys without a TTL are kept for good.** A configuration-built durable pool
  stores results without a TTL, so their idempotency keys accumulate. Start the
  pool with `DurableResourcePool::start` and `DurableOptions::with_result_ttl`
  if you need them to expire; the configuration has no field for it yet.
- A durable pool dispatches strictly in order (no skip-ahead).

Concepts: [Leases and delivery](../concepts/leases.md).
