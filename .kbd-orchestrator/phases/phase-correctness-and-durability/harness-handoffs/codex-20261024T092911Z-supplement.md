# Supplement to `codex-20261024T092911Z.md` (cross-machine handoff)

Written by the sending harness (Claude Code) on 2026-10-24 after 0.3.0 was
released. Read this right after the generated packet. Where the two disagree,
this file wins: the generated packet assumes the same machine.

## 1. The machine move

The canonical KBD journal is **not in git**. On the sending machine it lives at
`~/Library/Application Support/prometheus/kbd/projects/f6c384db-6ee1-4f1b-9bab-0e1602ec2615/`
(append-only `replicas/<id>/events.jsonl`, `project.loro`, `checkpoints/`).
Revision at handoff: **627** (last event `9cb19a43-6764-4b64-9cd9-c750bc8b83d2`,
hash `c0500242…`). The project identity is `.prometheus/project.json`
(`projectId f6c384db-6ee1-4f1b-9bab-0e1602ec2615`), which is tracked.

The operator carries over an archive of that directory
(`kbd-store-f6c384db-rev627.tar.gz`, with a `.sha256`). On the new machine:

1. `git clone https://github.com/Prometheus-AGS/prometheus-parking-lot-rs` and
   check out `main` (at or after the commit that added this file).
2. Verify the archive checksum, then extract it into the runtime's project store
   directory on that machine. On macOS that is the same path as above. On
   other platforms, ask `prometheus kbd projects` / the runtime docs for the
   data directory; **do not guess the path**.
3. `prometheus kbd register <checkout>` (the checkout declares
   `.prometheus/project.json`), then
   `timeout 30 prometheus kbd --path <checkout> status --json </dev/null`.
4. Confirm `revision` is 627 and `lastEventId` matches. If it is not, stop and
   report. Do not use `prometheus kbd migrate` to rebuild state from the
   projections unless the operator decides to. That path is a fallback, not
   equivalent, and it has not been tried.

**Unverified:** that `register` on a second machine attaches to the copied
store, rather than minting a new replica with empty state. Nobody has run this
before. Step 4 is the check. The sending harness has stopped issuing KBD
commands, so revision 627 is final from this side.

Paths in the generated packet (`/Users/gqadonis/Projects/...`,
`/Volumes/T7/...`) belong to the sending machine. Substitute your checkout path.

## 2. State at handoff (what is already done; do not redo)

- **0.3.0 is released.** All seven crates are on crates.io (facade
  `prometheus_parking_lot` plus `-core`, `-runtime-tokio`, `-pg`, `-embedded`,
  `-iggy` and `-wasm`; `-ffi` stays unpublished). There is an annotated,
  unsigned tag `v0.3.0` on `4b74735`. The GitHub release is
  https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/releases/tag/v0.3.0.
  docs.rs built all seven. Fresh-project installs from crates.io build for
  default, postgres, embedded, iggy, all three, and wasm32 without default
  features. Record: `.prometheus/decisions.md`, 2026-10-24 "0.3.0 released".
- **The final phase gate passed** (19 steps, `a3e517f`). Evidence:
  `evidence/2026-10-24-final-gate/` (README, `gate.sh`, per-step logs,
  `summary.txt`). Step 13 was corrected to the adopted secret-scan scope; see
  decisions.md, 2026-10-24.
- **The independent boundary review happened in two rounds.** Round 1 found 6
  CRITICAL; the re-review found 1 CRITICAL regression plus warnings. All were
  fixed in PRs #74, #75, #76 and #83. The PR bodies list each finding and its
  fix; they are the durable review record. Per the exit criteria (decisions.md,
  2026-10-24), there are no further review rounds for this phase.
- Implementation of every planned change is merged. The runtime counter reads
  **6 of 37** only because a change counts as complete after its last task, and
  each change still has its review task open (below).

## 3. Remaining work, in order

1. **Close the review task on the 35 changes** whose only open task is
   "Independent review at the merge gate": cr-16, cr-17, cr-18, cr-19, cr-20,
   cr-21, cr-22, cr-23, cr-25, cr-26, cr-27, cr-28, cr-29, cr-30, cr-31, cr-32,
   cr-33, cr-34, cr-35, cr-36, cr-37, cr-38, cr-39, cr-40, cr-41, cr-42, cr-43,
   cr-44, cr-45, cr-46, cr-47, cr-48, cr-49, cr-51 and cr-53. Use `kbd-apply
   begin-task`/`end-task` per task. The evidence is the review PRs above plus
   the final-gate evidence directory. Run `guard evaluate --boundary change
   --edge after` for each change as the packet's task rules describe.
2. **cr-52-release:** 1.4 (operator triggers the release) and 1.5
   (post-release verification) are done; close them with the release record as
   evidence. 1.1 (versioning ADR), 1.2 (trusted publishing) and 1.3 (release
   workflow) were deferred to 0.4.0 issues #86 and #85. Ask the operator
   whether to close them as deferred (with a KBD decision) or keep cr-52 open.
   Then close 1.6, the review task.
3. **cr-15-containment** is BLOCKED by operator decision
   (`phase-correctness-and-durability-defer-token-plans`) and tracked as #88.
   Tasks 1.1 and 1.3 are operator-only (credential rotation, GitHub Support
   purge). **Do not run `git filter-repo` or force-push.** Ask the operator how
   the phase treats cr-15 at close (deferred carry-over or a decision).
4. `kbd-apply verify` / `archive` for each closed change, then complete the
   Execute stage per `/kbd-execute`'s completion checklist.
5. **`/kbd-reflect`** for the phase. Lead with the delta between plan and
   delivery: the review loop needed explicit exit criteria; gate runs under
   load produced timing-only failures; crates.io rate-limits new crates. Seed
   the next phase from milestone 0.4.0 (#85–#89 plus the earlier gap issues).

## 4. Constraints that are easy to miss

- The operator merges every PR. Never merge your own.
- Never commit agent-tool credentials (`.codex/auth.json`, `.codex/sessions`,
  `history.jsonl`). `.codex/agents/*.toml` and `.codex/prompts/` are the
  tracked team definitions. Run `gitleaks protect --staged` before each commit.
- Never hand-edit KBD projections (`progress.json`, `current-waypoint.json`,
  `position.json`, `tasks.md` checkboxes). Mutate state only through `kbd-apply`
  / `prometheus kbd`.
- `.prometheus/` knowledge (wiki, decisions, gotchas) is tracked and
  append-only. Generated `.prompt-snapshots/project/generations/*.json` are
  not committed.
- Run builds and Docker suites only when nothing else is running
  (`.prometheus/gotchas.md`). Load caused every timing-only failure in this
  phase.
- `bindings/tauri-desktop/Cargo.lock` is a separate workspace and must be
  refreshed on every version bump.
- Rotating the leaked `.codex` credentials is the operator's job; until #88,
  they stay in public history.
