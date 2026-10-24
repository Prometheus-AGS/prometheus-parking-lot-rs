Scope: only the observed WASM doc_markdown lint repair, existing review/repair cycle 1.

Fresh source proof: crates/parking-lot-wasm/tests/wasm_pool.rs:8 now reads `//! pinned local \`ChromeDriver\`; Node remains the default.` The inspected git diff changes only ChromeDriver to its backticked Markdown form on this documentation line. No executable Rust, cfg, test identity, assertion or runner setting changes.

Verdict: documentation-only correction; no actionable issue in this delta. No tests, Cargo, builds, analyzers, production/source edits or broader review performed. Clippy acceptance remains unverified here and belongs to the parent's serialized gate. Only this owned review note was created.
