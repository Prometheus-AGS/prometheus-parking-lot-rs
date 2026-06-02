# CR-08 — Fix crate metadata + dual-license files (publish blocker)

- **Track:** C · **Severity:** 🔴 Critical · **Depends on:** none
- **Skill:** `workspace-structure` · **Agent:** general-purpose
- **Files:** `Cargo.toml`, `LICENSE-APACHE` (new), `LICENSE-MIT`

## Problem
`Cargo.toml` describes a **parking_lot synchronization fork**, not this scheduler:
- `description = "High-performance synchronization primitives…"` ❌
- `keywords = ["synchronization","mutex","rwlock","concurrent","prometheus"]` ❌
- `categories = ["concurrency","no-std"]` ❌ (crate is **not** no-std; it uses tokio/threads)
- License declares `MIT OR Apache-2.0` but **`LICENSE-APACHE` is missing**.

## Tasks
- [ ] Rewrite `description` to describe the parking-lot **scheduler**
- [ ] `keywords` → e.g. `scheduler`, `queue`, `gpu`, `ai`, `tokio` (max 5)
- [ ] `categories` → `asynchronous`, `concurrency` (drop `no-std`)
- [ ] Add `readme = "README.md"`, `homepage`, `documentation = "https://docs.rs/..."`
- [ ] Add `LICENSE-APACHE` file (standard Apache-2.0 text)
- [ ] Verify `repository` URL resolves; set `include`/`exclude` to trim package
- [ ] `[package.metadata.docs.rs]` with `all-features = true`

## Acceptance
- [ ] `cargo publish --dry-run` metadata clean
- [ ] Both license files present and referenced
