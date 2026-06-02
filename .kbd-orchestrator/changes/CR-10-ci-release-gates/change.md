# CR-10 — CI + release gates

- **Track:** C · **Severity:** 🟠 High · **Depends on:** CR-08, CR-09
- **Skill:** `rust-best-practices` · **Agent:** general-purpose
- **Files:** `.github/workflows/ci.yml`, `deny.toml`

## Tasks
- [ ] CI: `cargo fmt --check`
- [ ] CI: `cargo clippy --all-targets --all-features -D warnings` (pedantic/nursery)
- [ ] CI: `cargo test` matrix (stable + MSRV 1.91.1)
- [ ] CI: `cargo doc` (with `-D warnings`)
- [ ] CI: `cargo-deny` (licenses, advisories, bans) + `deny.toml`
- [ ] CI: optional `cargo-semver-checks` on release
- [ ] CI: `cargo llvm-cov` upload (ties to CR-04)
- [ ] Feature-matrix build (default / postgres / yaque / wasm target)

## Acceptance
- [ ] Green CI on a PR; all gates enforced
