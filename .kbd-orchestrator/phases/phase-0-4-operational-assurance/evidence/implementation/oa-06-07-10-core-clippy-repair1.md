# OA06/OA07/OA10 core Clippy targeted source repair 1

Source-only; repaired sources NOT_EXERCISED. Independent review correction cycle
remains1. Parent reports actual final-baseline candidate729cc425 Clippy exit101.
The actual stderr was read at /tmp/oa04-boundary-729cc42/baseline/clippy.stderr.txt:
three too_long_first_doc_paragraph diagnostics at conformance/mailbox.rs173/338
and conformance/task_queue.rs43, plus needless_collect at dispatch/parked.rs337.
This subagent did not execute Clippy or any verification command.

The latest central versions of exactly those three files were copied to the
techlead role tree before editing; exact source-base SHA hashes are retained
in the companion manifest and base-relative unified patch. The separate immutable
verification worktree/candidate was not touched.

- mailbox.rs: separate short first-paragraph summaries from tenant/user/session,
  None/empty distinction, tombstone and full-status/nonterminal-value details.
- task_queue.rs: separate its exact removed metadata/payload summary from live
  deadline equality and survivor ordering/availability details.
- parked.rs: retain the expired-key snapshot, whose iterator borrows entries
  immutably; use its length to preallocate the result vector, then explicitly
  remove/push each key in the same order after that borrow has ended. No mutable
  remove closure is attached to the borrowing entries iterator. self.remove
  remains the sole removal/accounting operation, and absent keys are skipped
  as with the previous filter_map. No blanket lint allow or expiry semantic
  change was introduced.

Only source reads, file authoring/copying, exact file hashing and unified-diff
generation ran. No tests, Cargo/build/Clippy, formatters, syntax checks, scans,
analyzers, review, runtime mutation, KBD/checkbox update, commit or push ran.
No unrelated additions were made; all edits trace to the four observed compiler
diagnostics. Compilation, Clippy acceptance and runtime equivalence remain
unverified until the driver-authorized next integration gate. The driver owns
candidate-bound source-map refresh/release re-preparation if required by the
new candidate; historical source-bound receipts must not be represented as
verification of these new bytes.
