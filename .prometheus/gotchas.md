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

## 2026-10-24T14:57:28.526385+00:00 — OA02 release preparation parser and candidate identity
The real pinned release-plz0.3.173 parser rejects duplicate historical version headings. Stage only the new generated section for set-version, then append original historical bytes after checking the new heading. Real preparation passed after this repair. Keep its pre-bump source72adf5d8ad583c3b52b22194563e94f7895c5b3f separate from finalpreparedcandidate729cc42549defb5fcf69b432485224cb89da74bc; retain original plan bytes/digest and validate ancestry+all13 file hashes.

2026-10-24 OA18: a cargo clippy --help invocation omitted the private RUSTUP_HOME/RUSTUP_AUTO_INSTALL environment and Rustup started syncing/downloading the numeric toolchain. Stopped only the observed owned PID43953 (help session exit143); no Rustup setting or credential change was made. Apply the private environment to help commands as well as builds.

2026-10-24 OA18 standalone binding boundaries: the13-file root release-preparation plan does not cover bindings/tauri-desktop/Cargo.lock. Inherited root version0.4.0 required updating exactly three no-source lock records; locked fetch observed101 before and0 after,434registry records unchanged. Root cargo fmt --all also does not format the excluded Tauri workspace; its own macOS profile caught four assurance.rs reflows. Verify standalone workspace gates at the completed boundary; do not infer them from root workspace PASS.

2026-10-24 OA19 observed CI defects: PG17/18 sequences have no composite row type; snapshot primitive last_value/log_cnt/is_called explicitly. Pinned wasm-bindgen-test-runner0.2.129 headless failures return1, not native libtest101; preserve separate successful build/discovery and exact browser assertion identity. Default hosted runners need explicit pinned browser, Tauri and upgrade image setup before strict validators; configured certification environments remain preprovisioned. All91normalCIchecks passed source2e65341; this is not full release certification.

OA20 merged-candidate certification: existing private Playwright cache /tmp/oa04-private-tools/playwright must be selected through PLAYWRIGHT_BROWSERS_PATH for website verification. Default macOS cache absence caused an actual launch failure; standalone production rerun passed, then fresh sealed baseline40PASS/0FAIL/2BLOCKED. Preserve initial failed receipt; do not edit it into PASS. Website Chrome156.0.8078.4 is not Linux OA13’s156.0.8078.47 proof.

Hosted assurance: offline validators require locked dependency prefetch for root and standalone workspaces before freeze. Cross-OS receipts need POSIX repository/packet identities and Git-index symlink-target semantics; native execution argv/cwd stay native. Observed independent-review failures fixed in OA20 task2; 17packet/9manual/18hosted/40containment checks passed, not final certification.

## 2026-10-25T15:23:04.157934+00:00 — 0.4.0 final boundary

Fresh anonymous Cargo homes do not put rustc on PATH. Pin the existing private toolchain bin explicitly; the initial postpublication resolve failed before compilation, and the corrected fresh native/WASM checks passed. Do not reuse HOME ancestor Cargo config or cached target when claiming fresh registry installability.
