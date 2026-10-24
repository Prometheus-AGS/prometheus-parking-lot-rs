# OA-18 targeted repair 1: frozen source tree scanner scope

Status: **NOT_EXERCISED**. Source authoring/static handoff only. The parent
assigned this observed baseline repair during the same cumulative correction
batch. The old `729cc` verification candidate/tree remains immutable; no file
there was edited. Driver owns model integration, updated freeze and real final
execution. No new operator approval flow or detector exception is introduced.

## Observed failure supplied by the parent

The existing `secrets-tree` row ran
`gitleaks detect --no-git --redact=100 --config .gitleaks.toml --source .`
after builds. The parent reported approximately 1.03GB scanned, including
ignored compiler/dependency/doc artifacts. This worker read only File/RuleID
metadata from the supplied redacted JSON diagnostic, not matches/source lines:

| Diagnostic File | RuleID |
|---|---|
| `target/debug/deps/libsqlx_postgres-825934e3e496aa85.rmeta` | `private-key` |
| `target/debug/deps/libsqlx_postgres-e68c659114830646.rmeta` | `private-key` |
| `target/debug/deps/libsqlx_postgres-1cbea82a066ca1a0.rmeta` | `private-key` |
| `target/debug/deps/libsqlx_postgres-0a342a9f408c2d16.rmeta` | `private-key` |
| `target/doc/search.index/3fbe7e5858.js` | `square-access-token` |

All five supplied locations are generated `target` files. The parent reported
no tracked-source findings and successful staged/ranged/pre-build tree scans;
this worker did not rerun or independently certify those scans. The source
row selected the current directory rather than the full frozen candidate's
tracked source. Removing detector rules or allowing all `target` paths would
hide a genuinely tracked canary and does not implement the requested scope.

## Exact source package

| Owned authored path | SHA256 |
|---|---|
| [.github/scripts/scan-candidate-tree.py](../../../../../.github/scripts/scan-candidate-tree.py) | `5045cd08e35e1f5d5604fe9802afba09c071de181beea19112e2ae54eacd0275` |
| [.github/scripts/tests/candidate-tree-fixtures.py](../../../../../.github/scripts/tests/candidate-tree-fixtures.py) | `903cb96f570e0801cfd5fa689783ab7c7f0b7b7e490f078e5df21a354fb3fe66` |
| [.github/scripts/tests/README-candidate-tree.md](../../../../../.github/scripts/tests/README-candidate-tree.md) | `82ebc21b2f3c91bbe820ece462e468faf0d5bd652f5342029693f538583bd4ea` |
| [.github/scripts/proposals/oa18-secrets-tree-source-scope.patch](../../../../../.github/scripts/proposals/oa18-secrets-tree-source-scope.patch) | `15a820115d91e8172b8c5cf08d567d948e21505a278704a9c191e733932cebc3` |
| This note | Exact final hash returned separately to the driver |

The helper resolves the full candidate commit, enumerates all root-recursive
tracked entries with NUL-delimited `ls-tree`, and materializes raw blobs through
`cat-file --batch` in a private temporary checkout outside both source and
exported packet. This includes tracked export-ignore paths and tracked files
under directory names such as `target`; it adds no path/rule/size exclusion.
Symlink blob text is scanned as a regular file, never dereferenced. Invalid
paths, gitlinks, missing objects and incomplete blob responses fail. Git
replacement objects and lazy promisor fetching are disabled. No checkout
filters or remote/LFS content retrieval occurs.

Real pinned gitleaks8.30.1 receives the explicit `.gitleaks.toml` materialized
from that exact candidate, `--no-git`, `--redact=100` and the complete temporary
source root. Actual scanner exit0 is PASS; nonzero actual scanner exits are
retained and returned, never converted into a source-scan PASS. Missing pinned
tools/version is BLOCKED2. The candidate's full source manifest, policy digest,
runner/tool identities, exact argv, actual scanner exit and raw redacted logs
are recorded. Exported finding metadata contains rule/path only. The full
finding JSON and source checkout remain private temporary inputs and are
removed on ordinary exit, never copied into the evidence packet.

## Required actual synthetic controls

The proposed row calls the fixture entry, which first scans the **actual frozen
candidate** and requires nonempty source/exit0. Its three additional real
Git/gitleaks controls then use a private owned fixture repository and the actual
candidate policy:

1. `ignored-build-artifact`: create a runtime-generated nonfunctional canary in
   ignored `target/debug/deps/generated-artifact.txt`. Require the clean tracked
   candidate to scan successfully, with the ignored decoy unchanged and absent
   from the scanned source metadata.
2. `tracked-same-canary`: commit the exact same bytes as `tracked-control.txt`,
   including a real `export-ignore` attribute. Require helper1/scanner1, FAIL,
   and actual `sk-prefixed-api-key` finding at that tracked path. Tool/config
   errors or a generic nonzero exit cannot satisfy the rule/path oracle.
3. `frozen-candidate-despite-later-worktree`: after the later canary commit and
   worktree exist, scan the original clean full commit and require success.
   This proves the requested candidate, not current HEAD/worktree, is selected.

No complete secret-like canary is tracked in source; it is assembled at
runtime from split prefix fragments plus a hash of a non-secret fixture label.
No scanner mock is used. Captured fixture output must not contain that value;
leaking owned output files are removed and invalidate the fixture. The wrapper
returns0 only when the actual candidate and all source-scope controls match.
No such result has been observed by this worker.

## Exact one-row shared-model proposal

Held `.github/operational-assurance.json` SHA256 when the proposal was authored:
`d77f170b21dc05a6fbde6e740290b82ce178ef94d947111d4bd35d6074e5824d`.
The linked patch changes only `commands[id=secrets-tree]` argv/tools; scenario
OA-05, baseline profile, timeout900 and all other rows remain unchanged:

```text
python3 .github/scripts/tests/candidate-tree-fixtures.py --repository {workspace} --candidate {candidate} --output {out}/secrets-tree
```

Required tools become python3, Git and gitleaks. The driver/tech-lead applies
this independently of other owners' model-row patches; this worker did not
write the shared model. The helper-only interface is:

```text
python3 .github/scripts/scan-candidate-tree.py --repository <frozen-root> --candidate <full-commit> --output <fresh-private-external-directory>
```

At final execution retain the wrapper receipt, actual candidate child receipt,
all three control receipts, `source-files.json` inventories and raw redacted
stdout/stderr. The source checkout/fixture repos/full finding report must not
be included. The runbook documents exact fields, failure semantics and the
success marker required after the actual checks.

## Work performed and remaining limitation

Performed: read current model/runner/scanner/fixture source; read only redacted
finding metadata; read installed primary Git manuals for actual command/output
APIs; authored helper, real-control fixtures, bounded model patch and runbook;
read SHA256 file identities. Git manual sources were
`/Library/Developer/CommandLineTools/usr/share/man/man1/git-ls-tree.1`,
`git-cat-file.1`, and `git.1` (`GIT_NO_LAZY_FETCH`). Existing real template
fixtures already use the pinned scanner report/redaction flags adopted here.

No helper/fixture/script, scanner, test, build, Cargo, syntax check, analyzer,
review, network/Docker/browser, KBD/checkbox action, commit, push or dispatch
was executed. No shared model, `.gitleaks.toml`, global allowlist, detector
pattern, root list or immutable verification tree was changed. Correctness,
redaction, cleanup and real control outputs remain unverified until the
parent's final frozen repair boundary. This scope fix certifies no history,
credential validity, operator gate or publication authority.
