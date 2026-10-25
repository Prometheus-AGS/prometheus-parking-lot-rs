# History containment: operator recipe and evidence

Owner: platform-engineer prepares; operator approves and executes G-02. Scope:
OA-01 / #88 in `phase-0-4-operational-assurance`. This is an unexecuted recipe,
not a purge receipt. Production roots are currently empty. No live ref inventory,
private backup, rewrite, remote update, cache removal or Support request was
performed while authoring it.

Authority is recorded in [operator-gates.md](../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/operator-gates.md).
G-01 credential rotation and exposed KBD signer replacement are explicitly
deferred and non-blocking for implementation and this release contract. These
steps do not neutralize copied credentials. G-02 still requires separately scoped
approval and observed evidence before a clean-history or release-certification
claim. G-05 publication remains separately approval-gated.

## Implemented commands and modes

Run [supply-chain.sh](../../.github/scripts/supply-chain.sh) from a non-bare
worktree. The script resolves that worktree's repository and reads its
[pre-purge-roots.txt](../../.github/pre-purge-roots.txt). A bare mirror alone cannot
run this entry point; create its detached verification worktree as described below.

| Command suffix after `bash .github/scripts/supply-chain.sh` | Empty, valid root list | Nonempty, valid root list |
|---|---|---|
| `tracked-credentials` | Blocking tracked-path check | Same blocking check |
| `secrets-diff "$RANGE"` | Blocking real gitleaks range scan | Same blocking scan |
| `pre-purge` | Reports unarmed; exits 0 | Rejects HEAD ancestry containing any recorded old commit |
| `path-history` | Reports findings/errors informationally | Rejects historical purged paths or inspection errors |
| `secrets-history` | Reports findings/errors informationally | Rejects real gitleaks findings or scanner errors |
| `release-containment` | Exits 1; refuses certification | Runs full-history, tracked-path, ancestry, path and secret checks |
| `package-allowlist` | Independent blocking Cargo package-list check | Same independent check |

Root records require full lowercase object IDs for this repository's hash
format, optionally followed by a `#` comment. Abbreviations, null IDs, malformed
fields, unreadable lists and existing non-commit objects fail. An old commit may
be absent from a fresh post-purge clone; successful complete ancestry traversal
is still required. Armed history checks reject shallow clones, legacy grafts
and traversal failures. Replacement objects are disabled. History/path scans
include all locally available refs and HEAD, with full history rather than
Git's default path simplification. They cannot discover a ref the server did
not expose or prove deletion of remote caches, forks or other clones.

Tracked paths include Codex credential/tool state, `.serena`, env and key files;
the existing `.env.example` exception remains. Historical path checks cover
`.codex/auth.json`, `.codex/history.jsonl`, `.codex/sessions`,
`.codex/config.toml` and `.codex/version.json`. Path logs emit commit IDs,
and gitleaks uses redaction. The operator does not open or print credential
values; scanners inspect content with redaction. The CI history steps no longer suppress nonzero exits; empty-root
informational behavior comes from the script itself.

The fixture command for the final phase boundary is:

```bash
bash .github/scripts/tests/containment-fixtures.sh
```

[The fixture script](../../.github/scripts/tests/containment-fixtures.sh) requires
installed real Git and gitleaks. CI currently pins gitleaks 8.30.1; record the
actual executable/version in the boundary packet. It creates isolated temporary
repositories, old refs, an orphan candidate, complete/shallow local clones,
discarded merge ancestry and runtime-generated nonfunctional canaries. Real
gitleaks handles clean and finding controls; explicit error shims cover tool
failure only. Each assertion requires its exact expected exit and oracle text.
A missing dependency, timeout, shell failure or absent final summary is not PASS.
The script prints its temporary raw logs before cleanup; the boundary runner must
capture all stdout/stderr and the final exit. No fixture or scanner has run yet.

## Before requesting G-02 approval: freeze the proposed operation

The following commands are instructions for a future operator session, after the
phase implementation set is complete. None has been executed for this document.
Use a private access-controlled location outside every checkout, public artifact
directory and cloud-synced share. The backup will contain known exposed history.
Record its exact destination and custodian before approval; never attach the
backup or credential-bearing blobs to an issue or Support ticket.

The operator must supply these absolute paths and a credential-free repository
URL. There are deliberately no guessed live hashes or example abbreviated roots.

```bash
set -euo pipefail
umask 077
: "${CONTAINMENT_PRIVATE_DIR:?Set the absolute private packet directory}"
: "${CONTAINMENT_PRIVATE_BACKUP:?Set the exact unused absolute backup mirror destination}"
: "${CONTAINMENT_REPO_URL:?Set the credential-free approved repository URL}"
mkdir -p "$CONTAINMENT_PRIVATE_DIR"
printf 'repository=%s\nprivate_packet=%s\nbackup_destination=%s\n' \
  "$CONTAINMENT_REPO_URL" "$CONTAINMENT_PRIVATE_DIR" "$CONTAINMENT_PRIVATE_BACKUP" \
  > "$CONTAINMENT_PRIVATE_DIR/operator-plan.txt"
git ls-remote --refs "$CONTAINMENT_REPO_URL" > "$CONTAINMENT_PRIVATE_DIR/live-refs.unsorted.tsv"
LC_ALL=C sort -k2,2 "$CONTAINMENT_PRIVATE_DIR/live-refs.unsorted.tsv" \
  > "$CONTAINMENT_PRIVATE_DIR/live-refs.before.tsv"
git ls-remote --symref "$CONTAINMENT_REPO_URL" HEAD \
  > "$CONTAINMENT_PRIVATE_DIR/live-head.before.txt"
```

These read-only remote inventories collect full advertised IDs. Do not treat a
local branch tip, this phase's baseline SHA or a stale remote-tracking ref as the
live value. Preserve the default-branch declaration and every advertised ref.
Build the approval table from `live-refs.before.tsv`: full ref name, full old
object ID, peeled commit ID where applicable, proposed action, protection owner,
and reason. Annotated tag object IDs and peeled commit IDs are different fields.
See [Git's ls-remote documentation](https://git-scm.com/docs/git-ls-remote).

The operator approval packet must also name:

- Repository owner/slug; exact inventory timestamp/digest; private backup path,
  storage protection, custodian and retention; rewrite and fresh-verification
  destinations, all distinct and unused.
- The five historical paths above, plus separately reviewed historical renamed
  paths if any are discovered. Do not silently broaden the deletion set.
- Every writable branch/tag action, excluded server-owned ref, affected published
  tag/signature, collaborator/fork owner and CI/cache surface. Rewriting signed
  history does not preserve its old signatures; signing authority is G-03.
- Write-freeze window, who may temporarily change and restore protections,
  reclone instructions, Support request scope and rollback authority. Restoring
  the old backup would restore the exposure and needs an explicit decision.
- Exact available Git/filter-repo/gitleaks versions. The planned
  `--sensitive-data-removal` operation needs git-filter-repo supporting that flag
  (GitHub specifies at least 2.47); absent tools are BLOCKED, with no automatic
  installation or global host change.

Request approval A naming this completed packet: private backup, disposable local
rewrite, proposed path/ref scope and reclone/cache plan. This approval must precede
clone or rewrite. It does not authorize any remote update or Support submission.
If any required field is unresolved, retain DRAFT status and do not execute.

## After approval A only: backup, rewrite and exact ref map

Recollect the live ref inventory under the agreed write freeze and compare it
with the approved snapshot. If it changed, stop and refresh approval. Clone the
backup at its recorded location, verify its advertised refs against the approved
inventory, and preserve an independently verifiable bundle before rewriting a
different fresh mirror:

```bash
: "${CONTAINMENT_REWRITE:?Set the approved unused absolute rewrite mirror destination}"
git clone --mirror "$CONTAINMENT_REPO_URL" "$CONTAINMENT_PRIVATE_BACKUP"
git --git-dir="$CONTAINMENT_PRIVATE_BACKUP" for-each-ref \
  --format='%(objectname) %(refname)' > "$CONTAINMENT_PRIVATE_DIR/backup-refs.txt"
git --git-dir="$CONTAINMENT_PRIVATE_BACKUP" bundle create \
  "$CONTAINMENT_PRIVATE_DIR/pre-purge.bundle" --all
git --git-dir="$CONTAINMENT_PRIVATE_BACKUP" bundle verify \
  "$CONTAINMENT_PRIVATE_DIR/pre-purge.bundle"
git clone --mirror --no-local "$CONTAINMENT_PRIVATE_BACKUP" "$CONTAINMENT_REWRITE"
(
  cd "$CONTAINMENT_REWRITE"
  git filter-repo --sensitive-data-removal --invert-paths \
    --path .codex/auth.json --path .codex/history.jsonl \
    --path .codex/sessions/ --path .codex/config.toml --path .codex/version.json
) > "$CONTAINMENT_PRIVATE_DIR/filter-repo.txt" 2>&1
cp "$CONTAINMENT_REWRITE/filter-repo/commit-map" "$CONTAINMENT_PRIVATE_DIR/commit-map.tsv"
cp "$CONTAINMENT_REWRITE/filter-repo/ref-map" "$CONTAINMENT_PRIVATE_DIR/ref-map.tsv"
cp "$CONTAINMENT_REWRITE/filter-repo/changed-refs" "$CONTAINMENT_PRIVATE_DIR/changed-refs.txt"
awk 'NR > 1 && $1 != $2 && $1 !~ /^0+$/ {print $1}' \
  "$CONTAINMENT_PRIVATE_DIR/commit-map.tsv" | LC_ALL=C sort -u \
  > "$CONTAINMENT_PRIVATE_DIR/proposed-pre-purge-roots.txt"
```

Preserve the untouched backup and bundle, their cryptographic digests, the
approved inventory, exact argv/exit and private tool output. A bundle protects
Git objects/refs; separately record LFS and any required non-Git backup scope.
Do not copy tool output into public evidence without checking its redaction.
The fresh-clone and bundle behavior is documented by
[Git clone](https://git-scm.com/docs/git-clone) and
[Git bundle](https://git-scm.com/docs/git-bundle).

The proposed root list uses the **old** column of changed commits, including
removed commits, never the new IDs. Review it against the backup and original
changed-history boundaries; it must be nonempty and contain only full old commit
IDs. This complete changed-commit set avoids guessing that one old tip covers
every diverged branch. Do not copy new first-changed IDs into the blocklist or
block unchanged shared ancestors. The tool's `commit-map`, `ref-map` and
`changed-refs` semantics are in the
[git-filter-repo manual](https://github.com/newren/git-filter-repo/blob/main/Documentation/git-filter-repo.txt).

In a disposable worktree on rewritten main, have the repository owner integrate
the reviewed old-ID list into `.github/pre-purge-roots.txt` and preserve the
candidate scripts/configuration. Record that new arming commit and transfer it
into the disposable rewrite mirror. Keep the rewritten tip and the later armed
candidate tip as separate values; the tool's initial ref-map does not include
the arming commit. Do not populate the current development checkout's real list
as part of this runbook task.

Prepare `approved-ref-updates.tsv` with one row per exact action: ref name, old
live object ID, rewritten object ID, final object ID after arming, action, owner.
Resolve deletions and tags explicitly. Preserve the original tool map alongside
this final operation map. Run local containment checks and the old-ancestry
control on the armed disposable candidate only at the final phase boundary;
record actual results, never expected results. A failed scanner may reveal an
additional path requiring a revised operation, not permission for a broader purge.

## Approval B: remote updates, reclones and Support

Request approval B naming the backup digest, final ref map, root-list digest,
full candidate SHA, exact remote argv, protection changes/restoration, reclone
and Support scope. Recheck live IDs immediately before acting; any drift stops
the operation. Every update must carry its explicit old-value lease. For one
approved update row, the command form is:

```bash
: "${CONTAINMENT_REF:?Set the exact approved full ref name}"
: "${CONTAINMENT_OLD_ID:?Set its exact frozen old object ID}"
: "${CONTAINMENT_NEW_ID:?Set its exact approved final object ID}"
git --git-dir="$CONTAINMENT_REWRITE" -c remote.origin.mirror=false push --atomic \
  "--force-with-lease=$CONTAINMENT_REF:$CONTAINMENT_OLD_ID" \
  "$CONTAINMENT_REPO_URL" "$CONTAINMENT_NEW_ID:$CONTAINMENT_REF"
```

Freeze the complete actual command list before approval. Prefer one supported
atomic batch containing all approved explicit leases/refspecs; if single-ref
operations are approved, record partial completion and stop on any failure.
The example above authorizes no deletion. Any deletion needs its own approved
ref and exact old lease. Do not use blanket `--force`, `--mirror`, wildcard pushes
or implicit remote-tracking leases. Server-owned PR refs are not writable push
targets. No protection bypass is automatic. See the
[explicit lease semantics](https://git-scm.com/docs/git-push).

Recollect every advertised ref afterward and compare full IDs with the final
approved operation map, including refs intentionally retained. Record failures
and exclusions. Restore protections and record their actual restoration. Direct
collaborators to fresh clones; quarantine old clones and private backups from
ordinary fetch/merge/push. Transfer reviewed patches onto the new ancestry rather
than merging old branches. Contact/fork cleanup and hosted CI/cache operations
require the approved owners and actual receipts; no broad cache deletion or
account mutation is implicit in this recipe.

For the separately approved Support request, prepare repository identity,
affected PR count/list, original first-changed commit IDs from the tool report,
and orphaned LFS metadata if present. Send no credential values or complete
backup. Record the ticket reference, submission time, exact requested cache/PR
cleanup, response and remaining exposed surfaces. GitHub documents that Support
may decline when credential rotation can mitigate the risk; a submitted ticket
does not prove cache deletion. Record the limitation as BLOCKED if evidence is
unavailable. G-01 remains deferred; do not turn it into an extra prerequisite.
See [GitHub's sensitive-data removal procedure](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository).

## Fresh-mirror and negative-control receipts at the final boundary

After approved remote work, clone from the actual remote, without depth limits,
alternates or a local old backup as its source. This mirror plus its detached
worktree retains all advertised refs for the history scans:

```bash
: "${CONTAINMENT_FRESH_MIRROR:?Set the approved unused fresh mirror destination}"
: "${CONTAINMENT_FRESH_WORKTREE:?Set its unused detached worktree destination}"
: "${CONTAINMENT_CANDIDATE:?Set the exact full armed candidate commit ID}"
git clone --mirror "$CONTAINMENT_REPO_URL" "$CONTAINMENT_FRESH_MIRROR"
git --git-dir="$CONTAINMENT_FRESH_MIRROR" worktree add --detach \
  "$CONTAINMENT_FRESH_WORKTREE" "$CONTAINMENT_CANDIDATE"
(
  cd "$CONTAINMENT_FRESH_WORKTREE"
  bash .github/scripts/supply-chain.sh release-containment
) > "$CONTAINMENT_PRIVATE_DIR/fresh-release-containment.txt" 2>&1
```

The expected successful message is `release-containment: clean-history checks
passed; operator purge/cache evidence is required separately`. It is an expected
oracle, not an observed result. Capture each command's actual exit even on
failure. Run the individual `tracked-credentials`, `pre-purge`, `path-history`
and `secrets-history` entries if separate scenario logs are required. Also run
the real fixture command above once at this boundary, capturing stdout/stderr
and its exit. The fixture's injected failures must be detected at their named
oracle; they are not failures of the successful suite.

In a separate private scratch clone of the clean candidate, create the actual
old-ancestry control without checking out historical credential files. Supply
an exact old commit from the private approved inventory/root mapping:

```bash
: "${CONTAINMENT_SCRATCH:?Set an unused private scratch destination}"
: "${CONTAINMENT_OLD_COMMIT:?Set an exact recorded pre-purge commit ID}"
git clone --no-local "$CONTAINMENT_FRESH_MIRROR" "$CONTAINMENT_SCRATCH"
git -C "$CONTAINMENT_SCRATCH" fetch --no-tags \
  "$CONTAINMENT_PRIVATE_BACKUP" "$CONTAINMENT_OLD_COMMIT"
clean_tree=$(git -C "$CONTAINMENT_SCRATCH" rev-parse "$CONTAINMENT_CANDIDATE^{tree}")
old_ancestry_control=$(printf 'Private old-ancestry negative control\n' | \
  git -C "$CONTAINMENT_SCRATCH" -c commit.gpgsign=false commit-tree \
    "$clean_tree" -p "$CONTAINMENT_OLD_COMMIT")
git -C "$CONTAINMENT_SCRATCH" -c core.hooksPath=/dev/null checkout --detach "$old_ancestry_control"
(
  cd "$CONTAINMENT_SCRATCH"
  bash .github/scripts/supply-chain.sh pre-purge
) > "$CONTAINMENT_PRIVATE_DIR/old-ancestry-control.txt" 2>&1
```

Capture the final command outside an enclosing `set -e` sequence or with explicit
exit capture: its required result is exit 1 with `HEAD descends from pre-purge
root`. A different failure is not a successful negative control. The clean tree
preserves the approved code and armed roots while the parent supplies real old
ancestry; [commit-tree](https://git-scm.com/docs/git-commit-tree) creates that
disposable commit. Never push this scratch history or reuse it for clean scans.
The fixture suite separately tests intentionally tracked credential paths and
real detector canaries using synthetic values only.

Bind receipts to full candidate SHA, adopted contract digest, root/ref-map
digests, tool versions, advertised-ref inventory and timestamps. Retain raw
redacted logs with exact argv/exit, nonzero fixture discovery and successful
oracle matches. Keep private backup paths behind an opaque receipt reference
in shared evidence. Missing approval/environment is BLOCKED; unattempted work
is NOT_EXERCISED; an oracle failure is FAIL. No empty-root exit 0, task count,
fixture-only success or Support request submission can certify G-02. The actual
fresh-mirror success, old-ancestry rejection and operator cache evidence are
still pending. Code completion, release certification and publication remain
separate outcomes.

### Superseding0.4.0 certification scope

Signed operator amendment04e74b62e7d21475021e682e36fcbbf4a6ddd7bb13a51103758590a6ab8916b2 defers actual repository history purge/cache removal and credential rotation beyond0.4.0. Known historical exposure remains unresolved; no clean-history or credential-retirement claim follows. Current candidate/package secret checks and disposable containment fixture negative controls remain blocking. Under this contractG02 refers to attributable deferral and those actual controls, rather than completed purge evidence. Historical receipts retain the earlier contract meaning. #88 must not be closed as purged.

Automated certification profiles use matching GitHub-hosted OS runners; actual macOSGUI and quiet performance evidence enters through independently approved sealed manual bundles. A build or an approval alone cannot replace GUI restart/retrieve, real benchmark measurements or any backend/browser/migration oracle. The collector still requires all seventeen scenarios, independent review, exact candidate/digest/raw evidence, public signing trust and authenticated publisher/environment authority. Original release preparation is preserved. New source/contract invalidates earlier complete-candidate receipts; do not relabel them.

The operator explicitly authorized publication after amended certification passes. SourcePRs remain for the operator to merge. Actual release-tag verification, protected-environment reviews and seven-crate OIDC settings remain required. No long-lived token fallback or signing/trust bypass is authorized. Detailed implementation/staging instructions accompany the hosted preparation helper; unarmed examples confer no authority.
