Candidate: `729cc42549defb5fcf69b432485224cb89da74bc`.

**Prior OA-08 recursion finding: resolved by static inspection. Runtime verification remains NOT_EXERCISED.** This is correction-only confirmation of cycle 1; no new review cycle or unrelated findings.

Freshly read code and caller proof:

- `crates/parking-lot-runtime-tokio/src/resource_pool.rs:844–845`: the unpolled guard's discard closure now calls `inner.discard(task, permit);`.
- `resource_pool.rs:892–897`: under one mutex, `discarded.pending.push_back((task, permit));` is followed by `if discarded.draining { return; }` and otherwise `discarded.draining = true;`.
- `resource_pool.rs:899–909`: one loop pops pending work; `discarded.pending.pop_front()` and the empty-to-idle assignment `discarded.draining = false;` occur under the same mutex. A concurrent enqueue therefore either joins the elected drainer or elects the next one.
- `resource_pool.rs:911–926`: each popped task has `SettleOnDrop(&self.ledger)`, and its failure event, `self.finish(...)`, payload destruction and callback panic containment execute outside the worklist lock.
- `resource_pool.rs:1011–1012` still calls `self.dispatcher.complete(permit, self.now_ms())` then `self.launch(woken, None)`; `launch:813` still hands the guarded future to `Spawn`. An inline successor drop now reaches the active-drainer early return above, so it cannot recursively settle another successor. The existing call chain remains bounded independently of backlog length.

I re-read the complete affected guard/drainer functions, initialization, launch/completion callers, backlog spawner and child regression, plus the correction patch, old-recursion control patch, implementation handoff and receipt JSON. The correction preserves ownership before the Spawn handoff and avoids reliance on another runtime-scheduled future.

Regression coverage is appropriate in source:

- `tests/discarded_spawn_test.rs:106–132` holds the first future, then drops each successor inline outside the spawner mutex.
- `discarded_spawn_test.rs:449–490` launches an isolated child, requires a successful exit and the settlement marker, and distinguishes its watchdog diagnostic.
- `discarded_spawn_test.rs:501–558` uses a fixed `2 * 1024 * 1024` stack and `TASKS = 16_384`, confirms one running/16,383 parked, begins shutdown, releases the retained future, and asserts zero execution, returned capacity and exactly 16,384 finished/zero unsettled ledger outcomes.
- `discarded_spawn_test.rs:561–586` checks one delivery per address and consume-once Failed(Cancelled) outcomes before printing the success marker.
- `discarded_spawn_test.rs:367–397` extends the existing callback-panic companion to three chained discards, checking that the drainer continues and each task reports once.
- The supplied old-recursion control patch restores the original production discard closure and removes the worklist while retaining the new regression. Its observed failure classifier and ordinary-spawner companion remain to be executed by the parent.

No Cargo, tests, builds, control application, production/source/index/HEAD/KBD changes, network actions or subagents were used in this confirmation. Only the two review records below were created. I do not claim that compilation, the backlog regression, the negative control, callback continuation or CI passed.

Created records in the central worktree:

- `.agent-team/reviews/oa04-runtime/initial-review.md` — exact initial report.
- `.agent-team/reviews/oa04-runtime/correction-confirmation.md` — exact confirmation report.

The prior source-level blocker is cleared; overall approval remains conditional on the parent's serialized runtime gates and green CI.

