# gotchas

Append-only. Dated entries. Mark superseded entries; do not delete them.

## 2026-10-22
- Initialized by prometheus-context-bootstrap.

## 2026-10-23 — crate `include` lists name READMEs that do not exist
- Six publishable crates (`core`, `runtime-tokio`, `pg`, `embedded`, `iggy`, `wasm`) list `/README.md` in `include`, but no such file exists. `cargo package` does not fail: a non-matching include entry is skipped, so the crates would publish with no README page on crates.io. Fix in cr-52: add a short README per crate and a `readme` key, then check `cargo package --list` for each.
- The local `wasm-bindgen-test-runner` in `~/.cargo/bin` is 0.2.126, but `Cargo.lock` pins wasm-bindgen 0.2.129. The runner rejects the mismatch, so install 0.2.129 (`cargo install wasm-bindgen-cli --version 0.2.129 --locked`) before running the wasm tests locally.

## 2026-10-23 — run the phase gate only on a quiet machine
- The second gate run (`f5e1b10`) ran alongside four fix agents and three reviewers, at a load average of about 400. Three Docker tests that had passed on the first run failed on timing alone: pgmq `leased_task_is_invisible` (a 1 s pgmq lease expired before the depth check), `shutdown_durable_pg_drain_restart_runs_the_rest` ("never reached its start state"), and Iggy `lifecycle_events_arrive_in_order` (7 messages instead of 6, probably a retried send that had already landed). Each test is being made robust. Run the boundary gate only when no agents or other builds are running.

## 2026-10-24 — Cross-machine KBD closure

- The rev627 journal archive restores canonical state but not the original Mac's Keychain signer. Registration and read-only status can succeed while the first mutation is refused. Verify the enrolled signer is available before starting task bookkeeping; use a private transfer, not a public content gateway.
- `kbd-apply end-task` evaluates and consumes the task-after and final change-after boundaries. Repeating change-after after a successful close creates a false blocker because the start receipt was already consumed. Inspect the saved receipt or canonical state instead.
- The managed OpenSpec runner has a global `operation.lock`. Serialize task listing, task boundaries, validation and archival even when changes and files differ. A contended listing can be retried after the known owner exits; do not remove a live lock.
- The transferred final-gate directory contains README, gate script and summary, but no raw `.log` files. Preserve that limitation when citing the sender's results; the summary is historical evidence, not a new local test run.

## 2026-10-24 — Source-derived negative-control classifier placement

- OA12 requires build/discovery plus a designated defect assertion; a watchdog, compile failure or missing-test exit cannot stand in. Existing discarded/panic/budget/slot fixtures can await a missing outcome before their later observable check. Put the existing production-path witness before the wait through its owner, retain positive assertions, and prove the final normal/control runs at the completed phase boundary. Source inspection is not proof those classifiers execute.
