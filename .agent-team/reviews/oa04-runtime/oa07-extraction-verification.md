Scope: OA07 execution_outcome extraction only, current central dirty source, existing repair/review cycle 1.

Verdict: no actionable issue in the extraction by static inspection. Behavior-preserving source repair; compiler/Clippy/runtime outcome remains NOT_EXERCISED by this reviewer.

Freshly read proof in crates/parking-lot-runtime-tokio/src/durable_pool/mod.rs:

- run:897 retains `self.release(permit);` before line 900's `self.execution_outcome(task_id, outcome, started)`. The release helper at 962–965 still completes the permit, launches admitted work and notifies the dispatcher before conversion/statistics.
- execution_outcome:918–946 contains the unchanged original match. Success increments `self.completed.fetch_add(1, Ordering::Relaxed)` and returns `(TaskStatus::Completed, Some(value), Resolution::Ack, Terminal::Completed(started))`. Failure increments failed with the same ordering, logs with the same task_id/error, computes the same error string, and returns `TaskStatus::Failed(error.clone())`, no payload, `Resolution::DeadLetter(reason)` and `Terminal::Failed(error)`. Full TaskError details and the original Option<Instant> are retained.
- run:901–912 constructs Settlement with the same task_id, mailbox, idempotency_key, status, payload, resolution, attempt and terminal, then awaits `self.settle(settlement, token, heartbeat, held, Some(reported))` unchanged. No new await, branch, return or fallible operation was introduced by the helper call.
- settle:1078–1099 still awaits delivery before normal lease resolution; failed delivery still requeues. Authoritative duplicate selection remains at 1106–1156. Terminal emission at 1159 precedes heartbeat drop at 1163, normal resolve at 1165 and held.finish at 1168. The pre-delivery resolve remains restricted to the existing s-16 negative-control cfg at 1075–1076.

Read the complete affected run/helper and relevant release/settle/deliver/resolve callers, exact source patch, current base-relative durable_pool diff and repair receipt JSON. The diff is limited to extracting the original local outcome/statistics match into this private helper plus its documentation; no delivery or lease-order code changed.

No tests, Cargo, builds, analyzers, production/source/index/HEAD/KBD changes, network actions or subagents performed. Only this owned review record was created. Parent's serialized compiler/Clippy and frozen runtime boundary remain required; no passing gate is claimed.
