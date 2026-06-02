# CR-12 — Final publish-readiness audit

- **Track:** C · **Severity:** 🟠 High · **Depends on:** all prior
- **Skill:** `prometheus-rust-auditor`, `unsafe-checker` · **Agent:** general-purpose

## Tasks
- [ ] `prometheus-rust-auditor` full pass; resolve findings
- [ ] `unsafe-checker` (unsafe already `deny`) — confirm no unsafe slipped in
- [ ] `cargo package` — inspect file list (no secrets, no stale docs, no `.firecrawl/`)
- [ ] `cargo publish --dry-run` succeeds
- [ ] Public-API review (`cargo public-api` / `cargo-semver-checks`)
- [ ] Set final version, tag, CHANGELOG release notes
- [ ] Follow `docs/PUBLISHING.md` runbook to publish

## Acceptance
- [ ] Dry-run succeeds; audit clean; crate ready for `cargo publish`
