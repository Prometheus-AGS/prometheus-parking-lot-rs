# OA18 operator decisions — local draft, not authorization

Hold this question until independently executable local checks and reviewer
follow-up finish. Frozen candidate:
`d6080b590a9663591066af4af15f1cb1453be7a2`. At drafting, the macOS profile is
running and native/redb/Loom27-case reruns are pending. Earlier iOS `68bc5f80d47119decc16de4ab70acdcbe2176042` PASS
and baseline `68bc5f80d47119decc16de4ab70acdcbe2176042` 40 PASS / 0 FAIL / 2 BLOCKED remain evidence for that earlier
candidate only. Insert actual final results before presenting this question;
this draft assigns no final disposition or certification result.

The successful [GitHub metadata audit](../environment-preparation/github-environments-readiness.json)
listed only `github-pages` and zero registered self-hosted runners. Existing
GitHub-hosted CI does not satisfy OA18's explicit self-hosted labels. Public
variable values/presence and installed runner/tool health were not inspected.

**Proposed single batched question:** Please approve, defer or amend each scope
below. None is authorized by this draft, repository admin access or the earlier
Execute instruction. Credential rotation and KBD signer replacement remain
explicitly deferred and nonblocking; no owner or rotation decision is requested.

1. **G02 Stage A: private backup and disposable local rewrite only.** Approve
   the [existing proposed scope](g02-proposed-ref-scope.json) and
   `docs/runbooks/history-containment.md`: reinventory under an agreed write
   freeze, then preserve an untouched full mirror plus verified bundle, and
   rewrite a separate disposable mirror for the five listed historical Codex
   paths. The recorded inventory has134 advertised refs (71 branches,62
   server-owned pull refs,1 tag), digest
   `031da0603315b133361951432235fba208526c563414de043f85ec3c6620372b`;
   it must be refreshed before acting, with drift stopping execution until the
   revised scope is approved. Proposed private operator-custodied root:
   `/Users/gqadonis/.local/state/prometheus/parking-lot-containment-20261024/`,
   with distinct `untouched.git`, `rewrite.git`, bundle/private records and
   `fresh-verification` destinations. Verify unused destinations, private
   storage/retention and required tool availability before proceeding. Keep
   backup blobs/credentials out of public artifacts. Return exact old/new ref
   maps and backup digests for review. This approves no remote update, force
   push, protection change, Support/cache-removal request or collaborator
   notification. Those remain separate Stage B after the concrete map and
   signed-tag/reclone/recovery effects are reviewed; Stage A alone cannot
   establish completed containment or G02 certification.

2. **Required certification hosts/environment.** Approve suitable existing or
   explicitly provisioned self-hosted resources/access covering labels
   `baseline`, `linux`, `macos`, `windows`, `android`, `ios`, `macos-gui`,
   `quiet` and `oa18-collector`, each paired with `self-hosted`. Linux needs
   real backend/browser collaborators; Windows its native build tools;
   macOS/iOS their native SDKs and actual manual macOS GUI observations. Quiet
   measurements require the documented absence of agents/builds/tuning, with
   real observations. Labels may share suitable hosts within serialization
   constraints; no nine-machine requirement is invented. Approve setup of
   `release-certification`, required reviewers and protected-main deployment
   policy. Specify any resource/cost limit; no paid jobs, installation,
   registration, settings mutation or remote dispatch is implied here.

3. **Public trust and independent authority.** Approve the exact final workflow
   source digests and candidate/contract-bound public policy under the existing
   `RELEASE_EVIDENCE.md` protocol. Supply approved existing public signing
   identity/trust configuration for G03, never private-key material. Separately
   approve protected configuration for `OA18_INPUTS_JSON`,
   `OA18_AUTHORITY_SHA256` and `RELEASE_TRUST_POLICY_JSON`, checking existing
   repository-level values before duplicating setup. Bind actual fresh review,
   G02/G03/G04 and GUI report/raw bytes through independently controlled digests;
   caller approval booleans are insufficient. G04 publisher/environment
   settings for the seven existing crates need attributable evidence. Rehearsal
   and certification require separately approved first-attempt workflow runs;
   this draft grants no dispatch or crates.io/account mutation permission.

4. **Eventual implementation delivery.** After actual local checks and review
   follow-up, approve an ordinary push of the final reviewed
   `codex/operational-assurance` branch to
   `Prometheus-AGS/prometheus-parking-lot-rs` and creation of a draft PR against
   `main`, naming the actual final source SHA and remaining external gates.
   Neither action is currently authorized. This scope excludes force push,
   merge, production tag creation, registry upload and GitHub release.

Implementation delivery, completed certification and publication remain separate.
External evidence stays BLOCKED/NOT_EXERCISED until actually obtained. G05 still
requires later explicit approval for the exact release candidate/tag and seven
publishable crates, excluding FFI; code/PR delivery does not require that
publication approval. No final certification or publication receipt is created
by this draft.
