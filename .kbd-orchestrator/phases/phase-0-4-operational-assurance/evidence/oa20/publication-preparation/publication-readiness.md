# Publication readiness: 0.4.0

Observed 2026-10-25 UTC. Read-only assessment under phase-0-4-operational-assurance / Execute. Native inherited model route; no model override or remote agent dispatch. Ownership limited to this private report directory. No source, KBD, credential, GitHub setting, Cargo invocation, tag, workflow dispatch, registry publication or configuration change occurred.

## Result

The seven-crate release cannot currently use the implemented trusted publication route. Crate ownership is established publicly; signing authority/availability, trusted-publisher setup and protected environment provenance remain unavailable or unverified. Deferring history cleanup removes a policy blocker only after the accepted contract/consumer are coherently amended. It does not produce signing or publishing authority.

## Observed facts

- All seven intended packages currently have only 0.3.0 published, unyanked. Their sole public owner is crates.io user GQAdonis (registry user id193208). Packages: prometheus-parking-lot-core, prometheus-parking-lot-runtime-tokio, prometheus-parking-lot-pg, prometheus-parking-lot-embedded, prometheus-parking-lot-iggy, prometheus-parking-lot-wasm, prometheus_parking_lot. FFI remains excluded.
- Authenticated GitHub identity is GQAdonis, user id100990, with admin/push/maintain permissions on Prometheus-AGS/prometheus-parking-lot-rs.
- Only GitHub environment github-pages exists (id25047369513). No release-certification, release-crates-io or release-signature-verification environment exists. Repository variable names are an empty list. Main is unprotected at ce034cb44d94bb4805ede4583a575f4e5215b728. release.yml has no workflow runs.
- No local v0.4.0 tag. Effective git signing metadata contains user.name/email only; no signing key, signing format or SSH allowed-signers path. ssh-add -l exits2: Could not open a connection to your authentication agent. This does not prove that no local signing device/private key exists; none was accessed.
- GitHub public signing-key metadata lists SSH signing ids1050763 (UAR release signing 2026-07),1196812 (the-boss laptop signing),1204954 (The Boss release signing 2026-09-28). No public GitHub GPG keys. A title is not approval for this release, and none is proven usable on this machine. Local .pub filename/size metadata is recorded only; file contents were not read and public-key format was not assumed from a filename.
- crates.io trusted publisher configuration was not established through the public versions/owner API. It remains unverified, not asserted absent. GitHub admin authentication is not crates.io session authentication.

## Implemented release route

.github/workflows/release.yml is workflow_dispatch only, requires main and explicit operation=publish. Validate job uses release-signature-verification environment and operator-pinned RELEASE_TRUST_POLICY_JSON. Publish job uses release-crates-io, real environment-review API provenance, id-token:write, pinned rust-lang/crates-io-auth-action3268eb413133e2960fa4ae0e963ca76380c54cd9, then the trusted-release.py gate. That gate validates an actual signed candidate tag v0.4.0, exact clean candidate, current contract digest, trusted successful first-attempt GitHub certification artifact and public signing trust before invoking cargo publish --workspace --exclude prometheus-parking-lot-ffi --locked --registry crates-io. There is no cached/long-lived token fallback.

The current unarmed .github/release-trust-policy.example.json has empty workflow digests/public signing fields and environment ids0/reviewers[]. Do not submit it as actual authority. Hosted runners also cannot read arbitrary existing workstation absolute allowed-signers paths: approved public trust must be provisioned reproducibly into each runner, without moving a private key.

operational-assurance.yml still uses self-hosted profile, quiet and oa18-collector labels. Merely approving hosted runners does not change those jobs. The hosted/manual contract and delivery workflow must be amended together, reviewed and merged before freezing a new final candidate; old candidate evidence remains historical.

## Smallest safe path

1. Root records the user's explicit G02/rotation deferral and hosted+attributable manual evidence decision in the accepted contract and publication-consumer requirements. Do not silently mark deferred containment PASS or drop signed-tag/OIDC provenance. Deliver changes through the user's merge process, then freeze the resulting main candidate.
2. Prepare exact reviewable GitHub settings payloads for protected release-certification and release-crates-io environments, required reviewer GitHub id100990 (prevent self-review considerations must match actual dispatching actor), main-only deployment policy; provision release-signature-verification public trust consistently. Configure/pin actual source digests, contract digest, environment ids/reviewer ids, approved public SSH signer fingerprint/principal and runner-local allowed-signers digest. Current repo admin session can technically perform GitHub settings changes, but this read-only worker has not done so. Root must assess current user authorization against settings boundaries.
3. Crates.io owner GQAdonis confirms/configures trusted publishing for every one of the seven existing crates: GitHub owner Prometheus-AGS, repository prometheus-parking-lot-rs, workflow release.yml, environment release-crates-io. Use authenticated crates.io UI or supported owner API; do not ask for API tokens/private keys in chat. Public crate ownership proves who can do it, not that this configuration exists.
4. Operator identifies an approved public signing key among existing registered keys (or another approved public reference) and signs the exact final candidate v0.4.0 on the machine/device that already has that signer. Transfer/push the signed tag/public verification material only. No new signer/private-key import or credential rotation is required by this recommendation.
5. Run remaining actual certification on that final candidate with independently approved environment history; collect a publishable GitHub artifact, rehearse the exact signed-tag/trust/artifact gate, then dispatch explicit publish under the existing user publication authorization. When GitHub requests an independent environment reviewer, the human must approve the actual job; a local recorded intention is not an API approval.
6. Verify all seven 0.4.0 versions and docs.rs outcomes after publication; if a registry publish partially fails inspect registry state before retrying.

The uncomfortable limitation: user approval to publish cannot create a missing hardware signer, establish crates.io owner-session access or substitute for real independent environment review. These are concrete present prerequisites, not credential-rotation blockers.

## Evidence and commands

local-and-github.json and identity-and-release-observation.json retain exact argv, exit status and filtered public metadata output. crates-public.json retains public GET results from https://crates.io/api/v1/crates/{name} and /owners for all seven names (all successful); these are public requests without auth/token headers. public-key-file-metadata.json retains names/sizes only. GitHub selectors intentionally omit variable values and public/private key bytes.

Source inspection: AGENTS.md; docs/agent-context.md; docs/skill-routing.md; docs/agent-team.md; .kbd-orchestrator/current-waypoint.json; docs/PUBLISHING.md sections2 and trusted-release protocol; .github/workflows/release.yml; .github/workflows/operational-assurance.yml; .github/scripts/trusted-release.py; .github/scripts/release_evidence.py; .github/release-trust-policy.example.json; package name/version/publish manifest fields. Source inspection is not execution evidence for publication.

Current official guidance: https://crates.io/docs/trusted-publishing and https://forge.rust-lang.org/infra/docs/trusted-publishing.html (read-only browser lookup). Rust Forge confirms GitHub Actions environment/workflow identity and id-token permission requirements; this external Prometheus organization must use crates.io setup rather than rust-lang/team ownership conventions.

Unverified: authenticated crates.io email/session and publisher entries; approved signing-key selection and actual signing capability; all unexercised/manual certification profiles; protected environment reviewer behavior; publication. No unrequested guards or production changes were added.
