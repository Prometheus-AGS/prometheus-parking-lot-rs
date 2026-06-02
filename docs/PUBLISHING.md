# Publishing `prometheus_parking_lot` to crates.io — Playbook

A repeatable runbook for releasing this crate publicly on [crates.io](https://crates.io)
with first-class [docs.rs](https://docs.rs) documentation. Follow it top to bottom for the
first release; for subsequent releases, start at **§5 Release**.

> Grounded in: the [Cargo book — Publishing](https://doc.rust-lang.org/cargo/reference/publishing.html),
> the community [crate release checklist](https://www.reddit.com/r/rust/comments/qw5ea1/),
> [crate publishing tips](https://users.rust-lang.org/t/blog-post-crate-publishing-tips-and-guidelines/39391),
> and [docs.rs/about](https://docs.rs/about).

---

## 0. One-time account setup

1. Create a crates.io account (GitHub OAuth) and **verify your email** — required to publish.
2. Generate a scoped API token: <https://crates.io/settings/tokens> (prefer a
   `publish-new` + `publish-update` scoped token).
3. Authenticate locally **without** persisting the token in shell history:
   ```bash
   cargo login   # paste token at the prompt
   ```
   In CI, use the `CARGO_REGISTRY_TOKEN` secret instead of `cargo login`.
4. Confirm the crate name `prometheus_parking_lot` is available / owned:
   ```bash
   cargo search prometheus_parking_lot
   ```

---

## 1. Pre-flight: metadata (`Cargo.toml`)

> ⚠️ Blocker today (see change **CR-08**): the manifest still describes a *synchronization
> primitives* fork. Fix before first publish.

Required / recommended fields:

```toml
[package]
name = "prometheus_parking_lot"
version = "0.1.0"                 # semver; see §5
edition = "2021"
rust-version = "1.91.1"           # MSRV — keep tested in CI
description = "Runtime-agnostic, resource-aware parking-lot scheduler for AI agent workloads."
documentation = "https://docs.rs/prometheus_parking_lot"
repository = "https://github.com/prometheus-ai/prometheus-parking-lot"
homepage = "https://github.com/prometheus-ai/prometheus-parking-lot"
readme = "README.md"
license = "MIT OR Apache-2.0"     # both LICENSE-* files MUST exist (CR-08)
keywords = ["scheduler", "queue", "tokio", "gpu", "ai"]   # max 5
categories = ["asynchronous", "concurrency"]              # NOT no-std
exclude = [".firecrawl/", ".kbd-orchestrator/", "docs/DISCUSSION.md", "reports/"]

[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
```

**Checklist**
- [ ] `description` reflects the scheduler, not parking_lot primitives
- [ ] `keywords` ≤ 5, lowercase, relevant
- [ ] `categories` are valid [crates.io slugs](https://crates.io/category_slugs)
- [ ] `repository` / `homepage` resolve
- [ ] `readme` present and accurate
- [ ] `exclude` trims dev cruft from the package (`.firecrawl/`, `.kbd-orchestrator/`, oversized docs)

---

## 2. Pre-flight: license

The manifest declares `MIT OR Apache-2.0`, so **both** files must ship:

- [ ] `LICENSE-MIT` ✅ (present)
- [ ] `LICENSE-APACHE` ❗ **add it** (standard Apache-2.0 text) — currently missing (CR-08)
- [ ] README footer states the dual license

```bash
# Sanity: both files exist
ls LICENSE-MIT LICENSE-APACHE
```

---

## 3. Pre-flight: documentation

Docs.rs builds your docs automatically from the published crate; make them clean locally first.

- [ ] `#![deny(missing_docs)]` passes (already enforced)
- [ ] Crate-root (`lib.rs`) overview: problem, features, backend selection, shutdown/Ctrl-C, hooks
- [ ] Public items documented with examples; prefer **runnable doctests** over `ignore`
- [ ] Intra-doc links resolve

```bash
cargo doc --no-deps --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features   # fail on doc warnings
cargo test --doc --all-features                                 # doctests
```

---

## 4. Pre-flight: quality gates

Run the full gate locally (mirrors CI / CR-10). **Do not publish on red.**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings        # pedantic/nursery on
cargo test --all-features                                       # must exit WITHOUT hang (CR-01)
cargo test --all-features -- --test-threads=1                   # confirm no runner hang
cargo +1.91.1 check --all-features                              # MSRV
cargo llvm-cov --all-features --workspace                       # coverage baseline (CR-04)
cargo deny check                                                # licenses/advisories/bans (CR-10)
cargo semver-checks check-release                               # API compatibility (updates only)
```

**Package inspection — what actually ships:**
```bash
cargo package --all-features --list      # review the file list
cargo package --all-features             # build the .crate without uploading
```
- [ ] No secrets, `.env`, `.firecrawl/`, `.kbd-orchestrator/`, or stale fork-era docs in the list
- [ ] Package size reasonable (large `docs/*.md` excluded)

---

## 5. Release

### 5.1 Version & changelog
- [ ] Bump `version` per [SemVer](https://semver.org/): pre-1.0 → breaking changes bump **minor**
- [ ] Update `docs/CHANGELOG.md` (Keep a Changelog format); move Unreleased → the new version
- [ ] Commit: `chore(release): vX.Y.Z`

### 5.2 Dry run (mandatory)
```bash
cargo publish --dry-run --all-features
```
- [ ] Succeeds with zero errors/warnings

### 5.3 Publish
```bash
cargo publish --all-features
```
> Publishes are **permanent** — a version can be *yanked* but never deleted or overwritten.
> Double-check the version before running.

### 5.4 Tag & verify
```bash
git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z
```
- [ ] Confirm the crate page renders: `https://crates.io/crates/prometheus_parking_lot`
- [ ] Confirm docs built: `https://docs.rs/prometheus_parking_lot` (watch the build log)

---

## 6. Post-release

- [ ] Add owners if needed: `cargo owner --add <github-user-or-team>`
- [ ] Announce (README badge, repo release notes)
- [ ] Open the next `Unreleased` changelog section
- [ ] If a release is broken: `cargo yank --version X.Y.Z` (and `--undo` to reverse).
      Yanking blocks new dependents but does not break existing lockfiles.

---

## 7. CI automation (recommended — CR-10)

Gate every PR and automate releases on tag:

- **PR workflow:** fmt → clippy → test (stable + MSRV) → doc → `cargo-deny` → coverage.
- **Release workflow:** on `v*` tag → dry-run → `cargo publish` using `CARGO_REGISTRY_TOKEN`
  secret. Consider [`release-plz`](https://github.com/release-plz/release-plz) to automate
  version bumps, changelog, and publishing from conventional commits.

---

## 8. First-release blocker checklist (from this phase)

Must be green before the **first** `cargo publish`:

- [ ] **CR-01** — worker-pool test suite no longer hangs the runner
- [ ] **CR-08** — manifest metadata fixed + `LICENSE-APACHE` added
- [ ] **CR-09** — docs/doctests clean, stale fork docs pruned
- [ ] **CR-10** — CI gates green
- [ ] **CR-12** — `cargo publish --dry-run` succeeds; audit clean
