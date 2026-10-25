Verdict: **Block runtime approval on one important correctness finding.** No critical issue established.

[crates/parking-lot-runtime-tokio/src/resource_pool.rs:836] Discarded-run settlement recursively processes the parked queue  
Severity: **Major / High**

Freshly read source:

- Line 836: `inner.finish(task.meta.id, permit, task.meta.mailbox.as_ref(), Err(error));`
- Lines 964–965: `let woken = self.dispatcher.complete(permit, self.now_ms());` followed by `self.launch(woken, None);`
- Line 796: `self.spawner.spawn(inner.supervise(task, permit));`
- `tests/discarded_spawn_test.rs:59` implements the supported discard behavior as `drop(future);`.

**Named failure scenario:** Construct a one-unit pool with a large prefilled `InMemoryQueue` and the existing `DiscardSpawner`, then call `shutdown_with`. The initial drain admits one task. Dropping its unpolled future synchronously calls `finish`, which admits and launches the next task; the spawner immediately drops that future, repeating the same call chain. Stack depth grows with queued task count. These calls cannot be tail calls: counters, delivery, ledger settlement and task destruction remain pending after each nested launch. A sufficiently large accepted queue therefore causes process-aborting stack overflow before its outcomes settle. `catch_unwind` does not contain stack overflow.

This also occurs without prefilling if a legal spawner retains the first future, subsequent submissions park, and the spawner drops subsequent futures inline when the retained future is released.

**Fix:** Process reentrant discard completions through a pool-owned iterative worklist with bounded stack depth. Deferring another future through the same discard-capable spawner would not establish settlement. Add a child-process regression with a substantial parked backlog, checking one outcome per task, returned capacity and zero unsettled work.

**Verification gates:** Read `UnpolledRun::drop`, `Inner::supervise`, `launch`, `finish`, `ensure_release_hook`, `ResourcePool::new`, submit/shutdown callers, dispatcher completion/drain, and the discard-spawner fixtures in full. Runtime manifest inherits workspace `edition = "2024"` and MSRV 1.95. This is accepted-work loss/process termination under the explicitly supported OA-08 behavior, not a style concern.

Strengths:

- Durable duplicate handling uses retained authoritative status for both terminal events and lease resolution, including consumed results; nonterminal fences requeue.
- Pruning returns original task metadata and payload, and the adapter performs fallible backend pruning before mutating its buffered front.
- Loom models exercise production ledger/semaphore code and explicitly document their bounded coverage.
- Iggy assurance includes strict poll-error propagation and healthy-writer controls alongside the poisoned-stream oracle.

Declined findings and limitations:

- I did not elevate callback-panic behavior in prefilled rejection/pruning into another finding: the candidate explicitly documents its existing callback limitations.
- No additional actionable defect established in the scoped WASM, native WorkerPool or Tauri runtime changes.
- Tokio is locked to 1.54.1. Runtime layer features include `rt-multi-thread`, `macros`, `sync`, `time`, and `signal`; native WorkerPool uses per-worker current-thread runtimes. No new blocking API pattern found in reviewed runtime async code, apart from explicit desktop fixture trace I/O.
- **No Cargo, tests, builds, runtime reproduction, network mutation or file modification performed**, as assigned. Stack-overflow finding is established by the synchronous call graph; the precise backlog threshold was not measured. Parent’s serialized checks and required green CI remain prerequisites to approval.

