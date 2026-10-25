Scope: only the observed embedded mailbox redundant_clone repair, within existing review/repair cycle 1.

Verdict: ownership is valid and behavior is preserved by static inspection. No actionable issue in this one-line repair.

Fresh exact proof in crates/parking-lot-embedded/src/mailbox.rs:

- deliver_tx:427–444 accepts `delivery: Delivery<T>` by value and constructs its locally owned Stored, moving delivery.status into stored.status.
- Line 445 executes `let bytes = serde_json::to_vec(&stored).map_err(backend(context))?;`. Serialization completes into owned bytes before the status move; it retains no borrow of stored.
- The duplicate branch at 459–462 only borrows `&stored.status` in duplicate_status and immediately returns its owned DeliveryOutcome. This branch never reaches the changed line.
- Lines 464–468 now serialize Remembered with `status: stored.status`. This is the last use of stored on the first-write path. Stored has no Drop implementation that prohibits the partial move. Lines 469–478 insert the owned remembered bytes and earlier entry bytes, then commit; they do not access stored again.
- The ResultMailbox::deliver caller at 596–602 moves its delivery into the blocking closure and deliver_tx; it requires no retained status or delivery value afterward.

Read the full deliver_tx function, its ResultMailbox caller, Stored/Remembered definitions, duplicate-status helper and supplied exact patch/receipt. The move preserves the same TaskStatus value, serialized entry/fence contents, duplicate behavior and transaction ordering; it only removes the redundant status clone.

No tests, Cargo, builds, analyzers, production/source edits or external actions performed. Only this owned review record was created. Compiler/Clippy/runtime success remains unverified here and belongs to the parent's serialized gate.
