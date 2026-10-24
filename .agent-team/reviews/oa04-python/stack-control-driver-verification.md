Narrow verification-orchestration review of `/tmp/oa04-run-stack-control.py` within the existing review cycle. No production Python change or general R2 re-review.

Scope: the complete temporary driver and the referenced backlog/ordinary-spawner test functions in the driver's actual worktree `/Users/gqadonis/.codex/worktrees/oa08-recursion-control/prometheus-parking-lot-rs`. The normal candidate's reported460-test PASS is parent-supplied context, not evidence independently rerun here.

Fresh classifier proof, `/tmp/oa04-run-stack-control.py:12-21`:

```python
assert result.returncode==expected,(label,result.returncode,expected)
```

```python
run("compile",[*base,"--no-run"],0)
listing=run("discovery",[*base,"--","--list"],0)
for test in ["discarded_spawn_large_backlog_is_stack_bounded","discarded_spawn_ordinary_spawner_preserves_success_panic_and_timeout_once"]:assert test+": test" in listing
negative=run("old-recursion",[*base,"discarded_spawn_large_backlog_is_stack_bounded","--","--exact","--nocapture","--test-threads=1"],101)
assert "OA-08:discarded-backlog: child did not settle" in negative and "stack overflow" in negative.lower()
assert "OA-08:discarded-backlog-watchdog" not in negative
companion=run("ordinary-spawner",[*base,"discarded_spawn_ordinary_spawner_preserves_success_panic_and_timeout_once","--","--exact","--nocapture","--test-threads=1"],0)
assert "1 passed; 0 failed" in companion
```

Verdict: this driver demands the requested prerequisites and discriminating oracles. Compilation and discovery must exit0 before the negative run; discovery must include both explicitly named test identities and therefore cannot be empty. Runtime invocations select their individually named tests with libtest `--exact`. The old-recursion run must exit101 and contain both the designated parent assertion and child stack-overflow diagnostic. The production test's own watchdog marker is explicitly forbidden. The outer `timeout900` wrapper at driver line9 also cannot satisfy the expected101 exit when it expires. The ordinary-spawner companion must exit0 and report one passed/zero failed before the driver can write its PASS classification at line22.

The driver-oracle correspondence was checked against the actual control worktree's `crates/parking-lot-runtime-tokio/tests/discarded_spawn_test.rs`:

- Lines455-465 launch the exact isolated backlog child. Lines472-477 emit the distinct watchdog failure. Lines483-487 emit `OA-08:discarded-backlog: child did not settle...` only when the child did not exit successfully and include its captured stdout/stderr. This ties the driver's required stack-overflow text to the failing child rather than treating every libtest101 as the intended defect.
- Lines403-444 define the exact ordinary-spawner companion. It enters through `ResourcePool::submit` with the real `TokioSpawner`, drains shutdown, checks completion/panic/timeout outcomes, consumes mailbox results once and checks one terminal event per task. Its exit0/one-pass requirement is a relevant unaffected production-path companion.

The driver records raw stdout/stderr separately and persists each actual argv/exit/start/elapsed value before classifying it. Its hardcoded candidate label and externally prepared worktree are parent-owned inputs; this small classifier is not independently a source-provenance attestation. This review confirms the decision predicates, not the worktree's historical-reversion identity or any eventual execution result.

Limits: only source reads were performed. The running driver was not interrupted, polled, altered or rerun. No tests, Cargo/builds, analyzers, production source changes, index/HEAD/KBD/network mutations or subagents ran. Python assertions are the driver's active classifier mechanism; this review does not claim behavior for an invocation that disables assertions. No execution PASS is claimed until the parent observes all actual receipts and final classification. The only file written is this requested central review artifact.
