# OA-18 boundary producer: source-only handoff

Execution status: NOT_EXERCISED. This runner, workflow, command definition and
fixtures are authored source. No tool health, build, discovery, oracle, remote
approval or release readiness is asserted by their presence.

The uncomfortable limit is external authority: current G-02/G-03/G-04,
independent review, actual macOS GUI observation, quiet-machine attestation,
preparation and multi-OS/tool availability may be absent. Available checks must
still run; the full packet keeps FAIL/BLOCKED/NOT_EXERCISED rather than turning
those absences into PASS. Credential rotation/signer replacement remain G-01
operator-deferred and nonblocking; copied credentials are not declared retired.

## Source and command authority

- `../operational-assurance.json` freezes the exact command definition, profiles,
  tools, specialised receipts, 52 OA12 cases and budgets. It is diagnostic packet
  coordination, not a second certificate schema.
- `operational-assurance.py` expands only checked inputs, probes preinstalled
  tools/images, freezes all tracked source/fixture SHA-256 identities and exact
  argv/environment before production execution, and runs one command at a time.
- `operational-assurance-gate.sh` is the adopted entry. Default native baseline
  is independent of Linux collaborators and missing release authority.
- `../workflows/operational-assurance.yml` is reserved for the approved producer.
  It uses only preprovisioned self-hosted labels, read-only permissions, protected
  main, first workflow_dispatch attempt and `release-certification`. Missing
  runners are unavailable evidence, with no hosted/paid/acquisition fallback.
- `tests/boundary-packet.py` authors actual verifier-predicate fixtures for stale
  candidate/contract, omitted OS/scenario, skip substitution, missing authority,
  nonexecuted rows and zero tests. It cannot certify real production behavior.

Existing `release_evidence.py` and `../release-boundary.schema.json` remain the
only compact artifact protocol. See [RELEASE_EVIDENCE.md](RELEASE_EVIDENCE.md).
No dependency, action version, global tool, host package, account, key or token
is installed/configured. Existing checkout/upload action versions are reused.
No publication, OIDC request, production tag/key mutation, history rewrite or remote dispatch
is performed by this runner. Existing OA03 fixtures intentionally create temporary
scratch tags/keys inside their owned disposable directory; they never establish
operator signing authority or change production trust/key state.

## Concrete proposed runtime interface

From a clean committed verification checkout, with owned evidence outside it:

```bash
bash .github/scripts/operational-assurance-gate.sh --candidate "$CANDIDATE" \
  --contract .kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json \
  --profile baseline --inputs "$PUBLIC_PROFILE_INPUTS" --out "$NEW_EXTERNAL_OUTPUT"
```

Use `--freeze-only` to record the expanded environment/command manifest and
NOT_EXERCISED before product gates. Version/identity probes are real subprocesses,
so even freeze-only is deferred to the authorized execution boundary. Each
profile/output is new; no stale receipt directory or implicit resume is used.
Profiles: baseline (native), linux (Linux collaborators/all52controls/browser),
macos/windows (actual native shell/build checks), android, ios, macos-gui and
quiet (the benchmark helper supports Linux/macOS). One host/target writer lock
serializes this runner; external Cargo users must also be stopped by the driver.
It does not claim to detect every unrelated process. A timeout/source change
preserves the lock for explicit owner diagnosis; never delete a live lock.

Public input JSON is a flat string map, only these keys are accepted:

| Keys | Meaning / existing owner entry |
| --- | --- |
| `rust_toolchain` | Explicit existing selector or absolute installed-toolchain path, verified as actual rustc/cargo 1.99.1 with executable SHA256. It can explicitly select existing stable bytes, never silently a different compiler. |
| `base`, `release_date`, `release_plz`, `git_cliff`, `preparation_source`, `preparation_plan` | Baseline OA02 uses a full pre-bump ancestor SHA, original plan path, full changelog base, ISO date and existing exact OA02 executable paths (0.3.173 / 2.14.2). The final candidate remains the job identity. Scratch ancestor preparation runs twice and must reproduce the original plan’s metadata/tool identities and the final candidate’s exact prepared files. |
| `chrome`, `chromedriver` | Existing pinned Linux x86_64 Chromium/driver 156.0.8078.47 paths, plus lock-matched wasm runner 0.2.129 and Node24 on PATH. |
| `pg17_image`, `pg18_image`, `upgrade_image` | Actual pre-existing immutable local IDs for the OA14 dual-binary rehearsal used by OA12's upgrade adapter. Ordinary PG17/18 baseline excludes binary(postgres_upgrade). No pull/build is implicit. |
| `fresh_mirror` | Separately approved post-purge full worktree clone for actual release-containment scans; G02 authority is independently required. No purge/fetch/rewrite is performed here. |
| `trust_policy`, `rehearsal_artifact_id` | Existing operator-controlled release policy and real remote approved rehearsal artifact for OA04's production fixture. Missing bootstrap provenance is BLOCKED. |
| `android_ndk` | Existing Android NDK root; cargo-ndk4.1.2, Kotlin2.4.20/JVM and target std libraries must already exist. No installer path. |
| `gui_positive`, `gui_control` | Existing owned actual macOS GUI roots from OA15 prepare/launch/capture workflow, with closed real process traces and manual captures. Classifier does not automate a fake GUI. |
| `baseline_root`, `quiet_attestation` | Isolated 0.3 baseline checkout; optional externally supplied truthful quiet attestation. The dedicated same-run workflow normally derives its own attestation from actual host/process observations after builds. Only external/manual attestation needs an independent digest binding. |
| `authority_config` | Existing independent report/raw-evidence binding described below. This file is not trusted merely because a caller points at it. |
| `tag`, `preparation_plan` | Collector uses the real operator-signed final candidate tag and the same exact original OA02 plan bytes, matching the final prepared candidate through validate_preparation. No regenerated plan is substituted. |

OA02 has two distinct commits. `preparation_source` is the original plan’s
pre-bump `candidate`, available as an ancestor in the final checkout’s Git object
store. `--candidate` on the outer gate always names the clean final prepared
commit. The checked OA02 argv passes `--candidate {preparation_source}` to the
incumbent preparation fixture and separately passes `--final-candidate {candidate}`
and `--preparation-plan {preparation_plan}`. The plan’s base/date must match the
frozen inputs. The shared validator checks its seven-package policy, ancestry
and every metadata hash against the final checkout before preparation runs.
Missing inputs block OA02 alone, allowing the other available checks to proceed.

The original plan bytes are frozen and retained as `original-release-plan.json`;
the fixture retains them again with separate real-tool logs and the newly
reproduced plan. Absolute source/tool/scratch argv paths differ between the two
invocations and remain truthful invocation provenance. They are never rewritten
to force byte equality. Portable plan fields, actual tool identity hashes/version
outputs and every prepared metadata byte must agree with the original plan and
final checkout. The reproduced plan itself must remain byte-identical on its
second run at the same output path. The compact artifact still copies only the
original plan, with its exact digest; the collector must use the same original
plan digest retained by the baseline freeze. Neither the reproduction nor a rebuilt plan
can replace it. Actual altered-plan controls reject metadata mismatch through
both the comparison and incumbent final-candidate validator.

The checked-in inputs example is an empty **profile map** for the workflow's
proposed `OA18_INPUTS_JSON` protected-environment variable; local `--inputs`
takes only one flat profile. Collector has its own map entry for accessible
external evidence paths. No token/config dump/command string is accepted.

`RUSTUP_AUTO_INSTALL=0` is frozen for every subprocess; cached dependencies are
used with CARGO_NET_OFFLINE=true. Exact compiler identity is version/hash based,
not inferred from a directory named 1.99.1. An explicitly selected installed
stable/absolute toolchain is allowed only when it actually reports 1.99.1.
Existing owner wrappers that contain `+1.99.1` keep that exact argv: its selector
has precedence over RUSTUP_TOOLCHAIN. The driver may separately provision a
private owned Rustup view using already installed exact bytes after the source
boundary, or the affected numeric-alias lanes stay BLOCKED. No inner command
rewriting, global alias/config mutation or automatic acquisition occurs.
Official semantics verified by the driver:
<https://rust-lang.github.io/rustup/environment-variables.html> and
<https://rust-lang.github.io/rustup/overrides.html>.
Tool identity probes distinguish absence, version mismatch and executable launch;
build/runtime health comes from subsequent actual gates. Required target SDKs,
caches, desktop libraries and browser installation remain environment prerequisites.
Website verification sets VERIFY_SKIP_INSTALL=1; frozen node_modules/browser
assets must already exist. No npm/global/browser installer is run.

## Per-job observations and full packet

Before running product commands, each profile writes checked-manifest.json:
full candidate/normalized adopted contract digest, definition digest, complete
tracked source/fixture map, expanded exact argv/cwd/public environment, explicit
input map/digest, OS/machine/Python, actual executable version/hash/process-health
proofs, installed image IDs and external authority dispositions. Raw version
stdout/stderr are retained separately. No environment dump or token is recorded.

Direct nextest/model rows also freeze exact discovery argv. Their real list output,
nonzero selected identities and raw hashes are recorded before actual execution,
with source identity rechecked between discovery and the run. Specialised owner
wrappers retain their incumbent pre-run build/discovery and exact classifiers.

Every command stores expanded argv, public selected environment, timestamps,
exit or timeout/unavailability, separate raw stdout/stderr and SHA256. Known
missing inputs/tools/images/authority are BLOCKED before attempting that check.
Unmatched exits/designated assertion/discovery failures are FAIL and retain raw
cause for diagnosis; a failing command is not by itself a proved product defect.
Timeouts cannot satisfy a negative oracle. Source/candidate changes abort the
remaining commands. No repair happens automatically.

Specialised scripts remain authoritative for their own discovery/classification:
OA12 normal build/list/run, injected assertion and unaffected companion, all52
rows and omission/defect-pass meta controls; OA13 real Node/browser21 cases;
OA14 real populated physical17->18 upgrade, independent backup restore and
missing-result control; OA15 native binary/trace/capture classifier; OA17 actual
80-process paired capture and10-process slow-control/report. Generic nextest,
libtest and unittest commands also require nonzero classified executed tests.
The two PG ordinary ignored legs exclude postgres_upgrade explicitly; only
OA14's marked owned upgraded cluster invokes its production upgrade test.

There is no invented speedup threshold: valid measured OA17 report execution
can PASS collection while reporting regression or overlapping noise; publication
judgment stays separate. Permanent generic Queued/Running fences retain the
OA07 requeue/availability limit. Populated migration backups remain digest-bound
source fixtures; frozen SQL/old-open probes are not published binary execution.

The boundary-specific predicate fixtures exercise production validate_packet:
stale candidate/contract -> OA-18:stale-packet; omitted OS -> omitted-os-receipt;
omitted scenario -> omitted-scenario; skipped status -> skipped-packet; a
BLOCKED/NOT_EXERCISED required result cannot leave global PASS. They supplement
rather than replace all real production/scenario and collaborator observations.

Local diagnostic assembly, without remote authority:

```bash
python3 .github/scripts/operational-assurance.py assemble --candidate "$CANDIDATE" \
  --receipts "$EXPORTED_PROFILE_PARENT" --inputs "$COLLECTOR_INPUTS" \
  --out "$NEW_EXTERNAL_PACKET"
```

Place exported folders under exact profile names. `verify_job` binds the exact
required command set, candidate/contract/definition/source/manifest, raw file
map, commands/exits/logs, nonzero tests and child receipt bytes. Native desktop
build receipts additionally require the exact OS and OA15-BUILD-LINUX/MACOS/WINDOWS
case identity at both execution and collector validation. Linux runs only in the
Linux profile; Mac and Windows retain their own required profiles. A PASS build
from another OS or another case cannot satisfy the missing native leg. Full packet
preserves every required slot and all four dispositions. Local source/data
alone never establishes remote provenance; no compact artifact is created.
Original per-profile records preserve independent baseline results even when
operator or other-OS evidence is absent.

## Proposed independent authority setup, deliberately unarmed

No operator report/settings setup exists merely because these example files
exist. The operator must separately approve the boundary workflow's exact SHA256,
protected main and release-certification environment/reviewer identities under
the incumbent RELEASE_TRUST_POLICY_JSON. That same protected environment is the
proposed source for OA18_INPUTS_JSON and a new OA18_AUTHORITY_SHA256 variable.
The latter pins exact bytes of an independently controlled authority-config
file available to the relevant host. Dispatch input JSON cannot set this pin.
No setup/account/credential mutation is authorized by this source delivery.

The unarmed authority example has no subjects. Actual config must bind full
candidate/contract and an explicit max_age_seconds<=604800. Required manual subjects are
review, G-02, G-03, G-04 and OA15-GUI. Optional OA17-QUIET is used only for
external/manual attestation, never as an extra approval gate for the automated
quiet job. Each supplied subject has an absolute report
path, SHA256, attributable independent source and nonempty evidence list of
absolute raw-file paths with SHA256. Report/raw bytes are retained in the packet;
public operator evidence must not include private keys, tokens or credentials.
No caller `approved: true` or standalone PASS file supplies authority.

Reports identify candidate/contract and timezone-aware observed_at; the approved
config's freshness limit is enforced. Additional observed facts are:

- review: actual native fresh R2 report plus raw outputs, verdict PASS,
  correction_cycles0/1 and open_critical_findings0. Builder is not its judge;
  this is an envelope binding actual host report bytes, not fabricated review.
- G02: report status PASS and approved exact rewrite plan/ref-plan/cache-removal
  evidence digests, with actual fresh-mirror command outcomes separately required.
- G03: report status PASS, approved tag and public_trust_sha256, plus actual
  verify_candidate_tag output against existing public trust. No scratch key
  fixture supplies operator signing authority.
- G04: report status PASS with authenticated read-only settings evidence and
  protected-environment receipt digests; OA04 real remote fixture/rehearsal is
  separately required. No publisher-account mutation is performed.
- OA15-GUI: independent verdict PASS and actual positive/control visual
  observation descriptions bound to original captures/trace/process evidence.
  Trace classifier alone remains pending manual observation review.
- Optional external OA17-QUIET: exact truthful machine/operator attestation
  bytes included among approved evidence. This is needed only when supplying
  external/manual quiet evidence, not for the dedicated automated profile.

After builds, the dedicated same-run quiet profile invokes observe-quiet. It
calls the incumbent real process snapshot/forbidden-name checks, additionally
rejects known benchmark/tuning processes, records raw snapshot/hash/load and
actual machine/workflow attribution, and derives the helper's attestation flags
from those observations and the workflow's no-tuning source. It performs no
kill/tuning/host change. The trusted collector later verifies its same-run
workflow/source/artifact provenance. No future automated output digest is
required before measurement. pg_measure rechecks processes/load for every
sample; unknown wrappers and between-snapshot activity remain explicit limits.
A local Codex host with a forbidden process is unsuitable and stays BLOCKED;
no user app is stopped to force quiet. An externally provided attestation uses
the independent optional binding instead. Automatic source-attributed evidence
and externally supplied manual approval are deliberately different inputs.

No future automated output digest is needed before a baseline/profile can run.
Authenticated same-run job artifacts obtain provenance from workflow/run/source
and immutable server archive digests, not from this external manual config.
A concrete first-attempt timing option is: run source-complete baseline/profile
jobs, retain visible immutable raw artifacts, perform actual independent manual
observations/review, then have the operator install the exact config pin before
approving the still-pending first-attempt collector environment job. If policy
protection uses one environment approval for the whole workflow, provide actual
review/observation evidence before dispatch or use a fresh independently
approved dispatch; never reuse a rerun's ambiguous approval history. This
configuration/review choreography is proposed, not an observed GitHub setup.
Gate-specific command inputs must exist before their profiles; a later config
approval cannot retroactively change an already BLOCKED command to PASS.

## Trusted collection and incumbent compact artifact

The collector's --trusted mode is for the actual approved first-attempt main
workflow only. It reads GitHub's fixed-origin run/approvals/artifact API, matches
workflow path, candidate/head repository, run ID/attempt, operator-pinned source
digest and environment/user IDs, and verifies each immutable job archive's
server SHA256. Missing raw artifacts leave named BLOCKED slots. A diagnostic
packet may still retain unapproved observed bytes, with provenance BLOCKED;
it cannot certify. Archive extraction rejects escaping/symlink/duplicate members
and preserves owned size limits; raw packets are capped separately from the
incumbent compact consumer's32MiB limit.

The collector invokes existing validate_preparation on the exact original OA02
plan bytes. The final candidate must actually be the prepared seven-crate0.4
workspace with matching manifest/lock/changelog digests and FFI excluded; a plan
made from a scratch0.4 copy does not certify an unchanged0.3 source candidate.
No fake tag or reconstructed preparation-plan fallback exists.

Actual tag/public trust, G02/G03/G04 and authenticated same-run provenance are
required even to emit compact evidence. Missing ones preserve the full packet
and disposition but omit compact. Certification additionally requires every
scenario, baseline, review and required profile to PASS. The compact artifact
is exactly boundary-receipt.json plus original release-plan.json, produced with
existing validate_receipt and uploaded once as operational-assurance-boundary.
Raw logs/job receipts and packet.json are a separate operational-assurance-packet
artifact. Prepared source copies/.git/build trees are excluded from exported
raw artifacts; original owned rehearsal files remain locally retained.

A deliberately requested rehearsal remains globally NOT_EXERCISED under the
shared schema and never authorizes publication. Its non-PASS compact records
use the schema's NOT_EXERCISED vocabulary; full packet.json retains their
actual FAIL/BLOCKED/NOT_EXERCISED values. No blocked authority becomes PASS.
The consumer independently verifies completed successful run provenance,
archive digest/freshness and public tag trust via fetch_boundary; local content
validation cannot authorize publishing. G05 publication approval is separate.

## Budgets, repairs and deferred final work

No automatic repair, reviewer dispatch or rerun exists. The driver owns two
targeted repairs per failing scenario, three no-progress attempts, the cumulative
2880-minute/3000-tool-call ceilings and one independent review/correction cycle.
Per-profile runner counters/time bound its own commands, but nested tools and
prior agent calls are not measurable by it; no fabricated total is emitted.
Preserve consumed budgets and failed artifacts, freeze corrected source only
after owner-coordinated hash refresh, and rerun only invalidated evidence.

The driver must use a separate clean committed verification checkout so KBD
projection writes do not alter source during checks. Any permitted formatting
repair changes source SHA/maps and requires a new matching freeze; no old
receipt may silently become current evidence. Missing collaborators/permissions
stay BLOCKED, not a request for unattended acquisition.

Task1 hands off source and exact hashes only. Task2 owns actual discovery,
production execution, fresh native R2 review/cumulative correction, observed
artifacts, final readiness and separate implementation/certification/publication
dispositions. No task2 check or release action ran while writing this document.
