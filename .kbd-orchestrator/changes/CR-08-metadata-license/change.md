# CR-08 — Fix crate metadata + dual-license files (publish blocker)

- **Track:** C · **Severity:** 🔴 Critical · **Depends on:** none
- **Skill:** `workspace-structure` · **Agent:** general-purpose
- **Files:** `Cargo.toml`, `LICENSE-APACHE` (new), `LICENSE-MIT`

## Problem
`Cargo.toml` describes a **parking_lot synchronization fork**, not this scheduler:
- `description = "High-performance synchronization primitives…"` ❌
- `keywords = ["synchronization","mutex","rwlock","concurrent","prometheus"]` ❌
- `categories = ["concurrency","no-std"]` ❌ (crate is **not** no-std; it uses tokio/threads)
- License: **MIT only** (per user decision 2026-06-02). Manifest previously declared
  `MIT OR Apache-2.0`; corrected to `MIT` — no `LICENSE-APACHE` needed.

## Tasks
- [x] Rewrite `description` to describe the parking-lot **scheduler**
- [x] `keywords` → `scheduler`, `queue`, `tokio`, `gpu`, `ai` (max 5)
- [x] `categories` → `asynchronous`, `concurrency` (drop `no-std`)
- [x] Add `readme = "README.md"`, `homepage`, `documentation`
- [x] Set `license = "MIT"`; keep `LICENSE-MIT`; fix README license footer to MIT-only
- [x] Set `exclude` to trim package (.kbd-orchestrator/, .firecrawl/, oversized docs)
- [x] `[package.metadata.docs.rs]` with `all-features = true`

## Acceptance
- [x] `cargo publish --dry-run` metadata clean (195 files; orchestrator/research excluded)
- [x] License field (MIT) matches the shipped `LICENSE-MIT`; README footer MIT-only
