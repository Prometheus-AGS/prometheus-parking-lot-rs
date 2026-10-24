# Frozen candidate source secret scan

Status at source handoff: **NOT_EXERCISED**. No helper, fixture, scan, build,
syntax check or review was run while authoring this repair.

The observed OA-18 baseline failure scanned the post-build workspace rather
than the frozen candidate's tracked source. The redacted diagnostic named four
`target/debug/deps/libsqlx_postgres-*.rmeta` private-key marker findings and one
`target/doc/search.index/3fbe7e5858.js` square-access-token finding. Those generated
artifacts do not define the candidate source tree. The parent reported that
staged, ranged and pre-build tree scans had passed; this worker did not rerun
those scans. This repair changes scope without adding detector exceptions.

[scan-candidate-tree.py](../scan-candidate-tree.py) scans every blob named by
`git ls-tree -r -z --full-tree <full-candidate>`. It reads bytes through Git's
`cat-file --batch` protocol into one owned private temporary checkout outside
the source repository and evidence packet. The full commit must resolve exactly.
No current index/worktree files are used, and no checkout filters, LFS fetch,
`export-ignore` attributes or directory-name exclusion can remove a tracked
blob. Git replacement objects and lazy promisor-object fetching are disabled.
A missing object or unsupported gitlink fails; it does not certify a partial
scan. The local repository must already have every required candidate object.

All regular tracked files, including binary files and files under paths named
`target`, are included if the candidate actually tracks them. A symlink is
materialized as a regular file containing its tracked Git link-target text.
The helper never follows the link into ignored or external files. Absolute,
traversal, drive/backslash and duplicate/unmaterializable paths fail rather
than escape the owned root or silently omit source. The candidate must contain
a regular `.gitleaks.toml` blob. Existing Git attributes are themselves scanned
as tracked source, but never alter the materialized set.

The real installed gitleaks 8.30.1 scans that temporary checkout with the explicit
candidate `.gitleaks.toml`, unchanged defaults/custom rules and no new allowlist,
pattern exclusion or maximum-size skip. Ambient scanner config cannot replace
the explicit policy. The helper records the actual scanner exit; 0 is PASS,
findings/tool failures are nonzero, missing required tools/version is BLOCKED2.
A malformed or missing finding report cannot supply a successful negative
fixture. This helper does not replace OA-18's clean-candidate/provenance gate,
staged/range/history checks, or operator G-02 evidence.

## Future commands

Use a fresh private absolute output directory outside the repository. These
commands are authored and have not been executed here:

```bash
python3 .github/scripts/scan-candidate-tree.py \
  --repository "$FROZEN_WORKSPACE" --candidate "$FULL_CANDIDATE" \
  --output "$PRIVATE_OUT/source-only"

python3 .github/scripts/tests/candidate-tree-fixtures.py \
  --repository "$FROZEN_WORKSPACE" --candidate "$FULL_CANDIDATE" \
  --output "$PRIVATE_OUT/secrets-tree"
```

The first invocation is the production helper alone. The second is the proposed
OA-18 `secrets-tree` row: it first requires the actual candidate's nonempty
source scan to exit 0, then executes three real Git/gitleaks controls in an owned
temporary fixture repository. It never commits, checks out or modifies the
actual candidate repository.

| Fixture identity | Required real observation |
|---|---|
| `frozen-candidate` | Actual requested candidate scan succeeds, scanner exit0, nonzero blob count |
| `ignored-build-artifact` | An ignored generated artifact contains a runtime-generated nonfunctional canary; the clean tracked candidate still scans successfully; the decoy stays unchanged and absent from source metadata |
| `tracked-same-canary` | Exactly the same canary bytes are committed as `tracked-control.txt`, marked `export-ignore`; helper and actual scanner exit1, finding rule `sk-prefixed-api-key` and path ending `tracked-control.txt` |
| `frozen-candidate-despite-later-worktree` | After the later canary commit/worktree exists, the original clean full commit still scans successfully, proving requested candidate scope |

The synthetic canary is assembled at runtime from separate prefix fragments
and a hash of a non-secret fixture label. No complete secret-like literal is
tracked. Controls use the actual candidate's policy and installed real scanner;
no scanner mock is used. The fixture checks that generated canaries are absent
from retained logs/metadata before emitting its captured output. If redaction
fails, offending owned output files are removed and the fixture fails.

The wrapper exits0 only after the actual source scan and all controls match.
An actual candidate scan failure propagates its nonzero exit/status; an
incorrect control result is FAIL1. Final success marker:
`PASS: frozen candidate source scan and all three real source-scope controls`.
No such result was observed during this source handoff.

## Evidence and ephemeral content

Each helper output retains `source-files.json` with path/mode/Git object ID/
byte count/SHA256 metadata, fully redacted `scanner.stdout.txt` and
`scanner.stderr.txt`, and `receipt.json` binding the full candidate, source
manifest/policy/runner hashes, tool version, exact scanner argv, actual exit,
log digests and finding rule/path metadata. Raw finding JSON is private and
temporary; source lines, matches and secret fields are not exported. The
fixture wrapper retains those child receipts and captured logs, plus its own
receipt naming actual control exits and child receipt digests.

The source checkout, fixture repositories and full scanner finding report are
created outside the exported packet and removed by owned temporary-directory
cleanup on ordinary exit. No archive or source copy is written to the packet.
Temporary source paths in argv are historical metadata, not exported source.
The gate's existing outer timeout bounds the command; a timeout or interrupted
helper is never a passing source scan.

The exact proposed model change is
[oa18-secrets-tree-source-scope.patch](../proposals/oa18-secrets-tree-source-scope.patch).
Only `commands[id=secrets-tree]` argv/tools change. Its scenario/profile/timeout
and every other row remain unchanged. The shared model is owned by the
tech-lead/driver; this worker supplied the patch and did not edit that model.
The immutable verification tree/current old candidate is not patched in place;
the driver includes approved source/model bytes in the next frozen repair
candidate before the final boundary.

Git protocol interfaces were checked in the installed primary manuals
`git-ls-tree(1)` (recursive/full-tree/NUL output/default fields), `git-cat-file(1)`
(batch object header, exact size bytes and trailing LF), and `git(1)`
(`GIT_NO_LAZY_FETCH`). Scanner argv follows the existing pinned production
`detect --no-git --redact=100 --config --source` interface, with report flags
already used by the real template-secret fixture. No new tool API or install
is introduced.
