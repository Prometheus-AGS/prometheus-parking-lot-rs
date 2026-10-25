# Hosted assurance preparation

The workflow runs six automated profiles on explicit ordinary hosted images:
`baseline`, `linux`, and `android` on `ubuntu-24.04`; `macos` and `ios` on
`macos-15`; `windows` on `windows-2025`. The collector uses `ubuntu-24.04`.
`macos-gui` and `quiet` are independently attributable manual bundles, never
headless hosted substitutes. Protected-main, first-dispatch and reviewed
`release-certification` environment requirements remain in force.

Setup occurs before the production producer freezes its tools and source. Rust
1.99.1, MSRV 1.95, nextest 0.9.149, cargo-deny 0.20.2, release-plz 0.3.173,
git-cliff 2.14.2, gitleaks 8.30.1, wasm-bindgen 0.2.129, cargo-ndk 4.1.2 and
Kotlin 2.4.20 are selected explicitly where needed. Node 24 follows the incumbent
major-version contract. The Linux browser helper uses the checked exact Chrome
and driver 156.0.8078.47 metadata. Website Playwright comes from the committed
npm lockfile; its separately provisioned browser is not the OA13 browser pin.
Validators remain noninstalling. Cargo invocations within each job are serial.
Before freeze, setup explicitly fetches locked workspace dependency sources for
all automated profiles, standalone UniFFI generator sources for baseline/Android/
iOS, and the desktop workspace sources through its actual `src-tauri/Cargo.toml`
for Linux/macOS/Windows. Fetch deliberately omits a target filter so locked
sources for native and cross targets are available on cache misses. Network
access is enabled only on these preparation steps; the production validator
retains `CARGO_NET_OFFLINE=true`. A restored build cache is an optimization,
never a prerequisite for a passing fresh hosted run.

`OA18_PLATFORM_JSON` supplies explicit public `android_ndk_version`,
`java_version`, `xcode_version`, and `xcode_build` pins. Android selects/installs
that exact SDK NDK package and checks installed Java. macOS and iOS select an already
installed matching Xcode with job-local `DEVELOPER_DIR`, checking its exact build.
There is no default to the moving newest NDK or Xcode. Missing public pins or
unavailable approved image tools fail setup rather than assert certification.
Linux library provisioning changes only the disposable hosted job. No workstation
installation, paid runner, persistent tuning or remote settings operation belongs
in these helpers.

## Immutable public material protocol

An operator-approved protected environment provides `OA18_MATERIALS_JSON`:

```json
{
  "schema_version": 1,
  "files": {
    "authority.json": {
      "artifact_id": 123,
      "artifact_digest": "sha256:<actual GitHub archive SHA-256>",
      "member": "public/authority.json",
      "sha256": "<actual authority.json SHA-256>",
      "profiles": ["baseline", "collector"]
    }
  }
}
```

The values above illustrate the schema and are deliberately not runnable pins.
Use real immutable GitHub artifact IDs in the same repository and independently
approved digests. Do not populate pins automatically from caller-provided or
candidate-provided metadata. The helper checks current artifact metadata, archive
bytes and selected member bytes independently. It rejects expired artifacts,
unsafe paths, links, encrypted ZIP members, duplicates, excessive sizes and
reserved destination names. It never extracts an archive wholesale or executes
material content. Download redirects use the incumbent fixed-origin GitHub API
client, which strips the read token when leaving GitHub API origin.

Materials stage outside the clean checkout at `RUNNER_TEMP/oa18-materials`.
Each manifest key is one normalized basename. Multiple selected files may refer
to different members of the same immutable artifact. Include authority reports,
every referenced raw evidence file, the original pre-bump preparation plan and
manual profile archives as necessary. Profile scopes limit downloads; manual
archives normally need only `collector`. The helper retains a public staging
receipt with the exact artifact/member/digest pins. It preserves original bytes,
including the authority configuration's bytes.

`OA18_INPUTS_JSON` retains its existing per-profile and `collector` mapping.
A path value `material:authority.json` resolves to the staged local file. Other
material input values use the same syntax, for example
`preparation_plan: "material:release-plan.json"`. The authority file's SHA-256
must independently equal `OA18_AUTHORITY_SHA256`; staging never rewrites this
file to fit runner paths. Authority report, evidence and manual archive paths
inside that pinned file are normalized basenames resolved by the collector
relative to the authority file's parent. Legacy signed/pinned absolute local
paths remain supported by the collector but are unsuitable as hosted examples.

`RELEASE_TRUST_POLICY_JSON` supplies the independently approved public trust
policy unchanged. Setup writes it to a job-local file. For baseline and collector,
`RELEASE_SSH_ALLOWED_SIGNERS` supplies independently approved public bytes at
the exact policy path `/tmp/prometheus-release-trust/allowed_signers`. Setup
checks the incumbent consumer digest over format, identity, fingerprint, exact
allowed-signers hash and explicit absent revocations before writing those bytes.
It does not rewrite the policy, derive trust from the candidate/tag, handle
private keys or silently omit configured revocations. Release jobs must stage
those same approved public bytes at that same path. Actual tool executable
and prepared Chrome/NDK paths are resolved into the job's input JSON; these
runtime input paths do not modify any authority report or pin.

Both baseline and Linux prepare wasm32 targets for 1.99.1 and 1.95. Before
Linux freeze, the workflow builds the source-pinned project PG17/PG18 images
and the physical-upgrade image using the incumbent ordinary-CI Docker path,
then pulls the exact checked Iggy digest. Actual local image IDs become frozen
inputs; validators do not build or pull missing images. macOS and Windows
Tauri builds compile the existing embedded static assets through Cargo, so
they do not require a Node frontend build. Native Rustfmt/Clippy components
are present on both; Linux installs the checked native development libraries.
Windows uses the standard hosted MSVC/Windows SDK installation, with its
actual native build retained as evidence rather than asserting a GUI launch.

The two manual entries in `authority_config.manual_profiles` are `macos-gui`
and `quiet`, each with `path` (staged archive basename), `sha256` (actual selected
archive file digest), and `subject` (`OA15-GUI` or `OA17-QUIET`). Their independently
pinned subject reports and raw observations must attest the actual archive bytes,
candidate, adopted contract and machine. The collector validates the sealed
producer export rather than treating an uploaded ZIP or a chat approval as PASS.
The manual archive digest is the selected ZIP file digest, distinct from the
outer GitHub artifact ZIP digest.

Missing real operator reports, signing identity, publisher authority, protected
settings or GUI/quiet observations remain prerequisites. This implementation
cannot manufacture them. Failed producer evidence is exported and uploaded;
the full collector packet retains FAIL/BLOCKED/NOT_EXERCISED outcomes, and only
an actually successful collector can upload the incumbent compact certificate.

## Verification boundary

After integrating the coherent collector/workflow set, run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 .github/scripts/tests/hosted-materials.py
```

This offline fixture drives the actual staging helper with real ZIP bytes and a
mocked read-only API transport. It checks immutable identity and path/ZIP attack
cases without installing tools, dispatching jobs, or substituting for the final
hosted production boundary. Actual hosted installations and all OS checks remain
unverified until the protected workflow runs on the final candidate.

The runner labels and immutable artifact metadata API follow the official
[runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
and [artifact REST API](https://docs.github.com/en/rest/actions/artifacts).
Pinned tool installation uses the existing
[install-action interface](https://github.com/taiki-e/install-action/blob/main/README.md).
NDK selection follows the official
[sdkmanager interface](https://developer.android.com/tools/sdkmanager); Xcode uses
Apple's [job-local developer-directory selection](https://developer.apple.com/documentation/xcode/configuring-command-line-tools-settings).

## Autonomous evidence preparation

Initial local public evidence enters Actions through the separately reviewed
[material transport](assurance-materials.md). It preserves original bytes and
creates no authority; the existing independent material/config pins remain
mandatory after the controller verifies the downloaded immutable artifact.

A `run_quiet: true` dispatch of the boundary workflow runs only a fresh,
dedicated `ubuntu-24.04` measurement job. It prepares the exact baseline,
source-pinned PG18 image and locked sources, then invokes the unchanged quiet
producer. The real same-workflow observer runs after both builds and the
measurement rechecks process/load/image identities per sample. No local coding
agent is stopped and no machine is tuned. Its sealed `oa18-quiet-measurement`
artifact is preparation evidence, never a certificate. Import it on a later
ordinary dispatch of the same candidate using the existing independently
approved manual-bundle mechanism. `run_quiet: false` remains the default and
runs the six automated profiles and full collector. A fresh hosted VM does not
prove an idle underlying physical host; the actual sampling oracles still have
to pass.
