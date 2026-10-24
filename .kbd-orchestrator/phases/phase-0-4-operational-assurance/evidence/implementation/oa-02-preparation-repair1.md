# OA02 preparation targeted repair attempt 1

Source-only delivery; execution of repaired sources: NOT_EXERCISED. This repair
addresses the parent-observed real OA02 failure. Final review correction cycle
remains1; this is targeted scenario repair1, not another review cycle.

Parent-reported command at source commit16db521829d83ba140bb5ce2ea8f8d3252acf35d:

```text
timeout 600 python3 scripts/release/prepare.py prepare --source CENTRAL --output /tmp/oa04-preparation-16db521 --release-plz /tmp/oa04-private-tools/bin/release-plz --git-cliff /tmp/oa04-private-tools/bin/git-cliff --candidate 16db521829d83ba140bb5ce2ea8f8d3252acf35d --base 92da0d3d28f7896ddc67062fa49ba40ea48d74a3 --version 0.4.0 --date 2026-10-24
```

CENTRAL means /Users/gqadonis/.codex/worktrees/operational-assurance/prometheus-parking-lot-rs.
Parent observed EXIT1 from actual release-plz0.3.173: failed to update changelog
at CHANGELOG.md; can't parse changelog; multiple release notes for '0.3.0'.
This subagent did not run that command or independently reproduce its output.

## Source-derived cause and minimum production change

Pinned release_plz_core0.41.0 src/command/set_version.rs update_changelog reads
the entire changelog, calls last_release_from_str, then replaces only the
latest title's old version. ChangelogParser::new calls parse_changelog::parse;
parse-changelog0.6.17 src/lib.rs parse inserts versions into an IndexMap and
returns the exact duplicate-release diagnostic on a repeated key. Shared path
deduplication in update_package_changelog does not avoid this parser requirement.
These cached primary source files were read directly; no executable ran.
Official tagged source reference:
https://github.com/release-plz/release-plz/blob/release-plz-v0.3.173/crates/release_plz_core/src/command/set_version.rs

prepare.py previously passed both its temporary current-version section and the
candidate's historical section for that same version to this parser. It now
passes only the generated seven-package current-version section to the unchanged
real set-version argv. After the existing requested-heading check succeeds, it
appends the immutable candidate's historical body with the existing header/leading
whitespace treatment. Historical release entries/prose are neither renamed nor
deleted. Both first and second preparation re-stage the isolated generated
section; second-run current metadata already equals target and remains covered
by existing deterministic plan/file checks. No fallback, parser replacement,
flag, dependency/pin/version change or publication step was introduced.

## Authored real-fixture assertion

The existing test-preparation.py fixture still invokes the actual preparation
and pinned tools twice. It now reads the ancestor's raw changelog stdout without
stripping trailing bytes, requires the final changelog to end in exactly that
historical body, and requires its level2 heading list to equal the single new
version/date followed by every historical heading in original order. The source
already contains the published0.3.0 section that triggered the actual failure.
The existing seven-package subsection assertion, source-unchanged check,
mismatch guards and byte-identical second-run plan/output assertions remain.
ADR0005 and the OA18 runbook now describe this staging order accurately.

## Verification and next boundary

Only targeted source reads, file copies/authoring, unified-diff generation and
exact SHA hashing ran. No repaired script, release tool, test, Cargo process,
build, syntax check, formatter, scan, analyzer or reviewer ran. The driver owns
the next serial real preparation from a new committed source candidate and fresh
output; the old failed output/commit must not be presented as a success plan.
No KBD/checkbox, commit/push, runtime, remote, credential or account mutation.
Only the four listed production/fixture/runbook files and this evidence packet
were changed. No unrelated guard was added. Historical preservation and actual
pinned-tool compatibility remain unverified until that real invocation succeeds.
