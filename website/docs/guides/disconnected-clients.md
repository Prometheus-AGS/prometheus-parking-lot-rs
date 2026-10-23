---
title: Disconnected clients
description: Deliver every outcome to a mailbox address so a client that closed the tab, lost the network or was asleep collects it later, once, even across a service restart.
---

# Disconnected clients

**Use it when** the client that submits work may not be connected when the
work finishes: a browser tab that closes, a phone that changes networks, a
desktop app put to sleep, an agent step that outlives its HTTP request.

**What you get:** each task names a mailbox address (tenant, user, session).
The pool stores the outcome there, success or failure, whether anyone is
listening or not. The client returns with the same address and consumes each
outcome once. On a durable backend the outcome also survives a restart of the
service.

Full source: [`examples/disconnected_client.rs`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/disconnected_client.rs).
It uses the embedded redb backend and needs no server:

```bash
cargo run --example disconnected_client --features embedded
```

## The address is the contract

A `MailboxKey` is the address. Give each user session its own, so two tabs of
one user do not take each other's results.

```rust title="examples/disconnected_client.rs"
/// The address a client reads its results from: one per user session.
fn address(user: &str, session: &str) -> MailboxKey {
    MailboxKey {
        tenant: "acme".into(),
        user_id: Some(user.into()),
        session_id: Some(session.into()),
    }
}
```

The task carries it, together with an idempotency key that makes a re-run
store one result, not two:

```rust title="examples/disconnected_client.rs"
            mailbox: Some(to.clone()),
```

```rust title="examples/disconnected_client.rs"
            idempotency_key: Some(format!("summary-{id}")),
```

## Submit, go away, come back

The service is a pool on a redb file. `submit` returns once the task is stored,
so the service can stop at any point after it.

```rust title="examples/disconnected_client.rs"
    {
        let service = start_service(&path).await?;
        let pool = &service.durable_pools["summaries"];
        pool.submit(task(1, &alice, "the quick brown fox")).await?;
        pool.submit(task(2, &alice, "")).await?;
        pool.submit(task(3, &bob, "jumps over the lazy dog today"))
            .await?;
        let report = pool
            .shutdown_with(ShutdownPolicy::Drain {
                deadline: Duration::from_secs(10),
            })
            .await;
        assert_eq!(report.unsettled, 0, "{report:?}");
        println!("service stopped: {report:?}");
    }
```

After the restart, the client reads its mailbox. A failed task arrives as
`TaskStatus::Failed` with the error, not as silence:

```rust title="examples/disconnected_client.rs"
    let service = start_service(&path).await?;
    let pool = &service.durable_pools["summaries"];
    let mut alice_seen = Vec::new();
    for _ in 0..500 {
        while let Some(entry) = pool.consume(&alice).await? {
            println!(
                "alice: task {} -> {:?} {:?}",
                entry.task_id, entry.status, entry.payload
            );
            alice_seen.push((entry.task_id, entry.status));
        }
        if alice_seen.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
```

```text
service stopped: ShutdownReport { completed: 0, cancelled: 1, recorded: 0, requeued: 1, dropped: 0, unanswered: 0, unsettled: 0 }
alice: task 1 -> Completed Some("4 words")
alice: task 2 -> Failed(Executor { message: "empty document", retryable: false }) None
bob: task 3 -> Some("6 words")
```

`consume` takes the oldest outcome under the address and removes it: a second
read returns `None`, and one user never sees another's results.

## Three levels, pick one

| Pool and mailbox | Survives a client disconnect | Survives a service restart | Client learns the result is ready |
| --- | --- | --- | --- |
| `ResourcePool` with an in-memory mailbox ([README example](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs#example-resourcepool-with-a-mailbox)) | yes | no | by polling |
| Durable pool on Postgres or redb (this guide, [Cloud Postgres](./cloud-postgres.md)) | yes | yes | by polling |
| Durable pool whose mailbox notifies through Iggy ([Iggy events](./iggy-events.md)) | yes | yes | a `mailbox.ready` event |

`WorkerPool` is different: its results wait in per-task slots for `retrieve`
(with a TTL on native targets), and they do not survive a restart.

## What this does not do

- **Polling is the default.** Without a notifier the client must ask. Poll with
  a backoff, or add the Iggy notifier.
- **Results from a configuration-built durable pool never expire.** They wait
  until consumed. Set a TTL with `DurableOptions::with_result_ttl` when you
  start a `DurableResourcePool` yourself; the configuration has no field for it
  yet.
- **Consume-once is per read, not per device.** If two devices of one user
  share an address, whichever reads first gets the result. Give each device or
  session its own address.

Concepts: [Mailboxes and actors](../concepts/mailboxes-and-actors.md).
