# Initial public assurance material transport

`assurance-materials.yml` closes the bootstrap gap before the existing immutable
material downloader can run. It transports bytes; it cannot establish that an
authority report is true, approve an operator gate, or certify a release. A
successful transport must never be reported as a passing assurance boundary.

Prepare one ZIP outside the source checkout containing only reviewed public
evidence, using flat safe basenames. Preserve the original pre-bump preparation
plan bytes, source-bound reports and their raw evidence. A sealed manual profile
export can be a nested ZIP file under one basename; its internal structure is
validated later by the incumbent collector. Do not include credentials, private
keys, normal app data or unreviewed screenshots. Run the required local secret
scan before uploading. A draft release is merely a temporary same-repository
transport source and must not be published as a software release.

Upload the seed ZIP to an explicitly owned draft release using the normal
GitHub release-asset interface. Record its real numeric asset ID and compute
SHA256 over the exact local ZIP bytes. Dispatch **Assurance public material
transport** from protected `main` with `candidate` equal to that workflow commit,
`asset_id` equal to the actual same-repository asset ID, and `archive_sha256`
equal to the independently computed digest. The existing reviewed
`release-certification` environment and first-attempt restriction remain in
force. The job has `contents: read`; it cannot create releases, publish software,
change settings or mint publishing authority.

The helper requests only the fixed current repository's release-asset REST
endpoint. It validates asset ID, canonical metadata URL, uploaded state, size,
server digest and independently supplied digest, then requests binary bytes
with `Accept: application/octet-stream`. The shared redirect handler accepts
only HTTPS and removes Authorization on redirects away from `api.github.com`.
Diagnostics omit URLs, tokens and supplied content.

The archive is limited to 32 MiB compressed, 64 MiB expanded and 1,000 regular
file members. Member names are ASCII basenames of at most 128 characters,
starting with an alphanumeric character; paths, hidden names, duplicate or
case-colliding names, links, special files, encrypted entries and supplied
`seed-transport-receipt.json` are rejected. Only stored/deflated ZIP members are
accepted. The output directory must be fresh. The helper never executes the
files, interprets evidence reports or rewrites their bytes. These guards protect
the actual untrusted release-asset/ZIP boundary.

The uploaded immutable Actions artifact is named
`oa18-public-materials-<run ID>-<attempt>`, with seven-day retention and no
overwrite. Its `seed-transport-receipt.json` records the workflow candidate,
source asset/digest and exact per-file hashes with status `TRANSPORT_ONLY` and
authority `NOT_ESTABLISHED`. The workflow summary exposes the actual artifact ID
and archive digest returned by `actions/upload-artifact@v7`.

Download that Actions artifact locally and verify every selected member against
the original local bytes before configuring `OA18_MATERIALS_JSON`. Use the
artifact API's actual immutable archive digest, not the source seed ZIP digest;
they identify different ZIP files. Then independently pin the existing authority
configuration via `OA18_AUTHORITY_SHA256` and configure `OA18_INPUTS_JSON` using
`material:<basename>` paths. `prepare-hosted-assurance.py` and the collector keep
their existing independent pins, public policy and evidence validation.

At the completed source boundary, the driver runs:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 .github/scripts/tests/assurance-seed.py -v
```

The fixture exercises the actual decoder and staged download path with real ZIP
bytes and a mocked read-only API transport. It does not dispatch a hosted job or
claim successful draft-asset access by `GITHUB_TOKEN`. That actual permission
and upload behavior remains unverified until the protected workflow runs.

References: GitHub's [release-asset REST interface](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)
and the official [upload action outputs and retention interface](https://github.com/actions/upload-artifact#outputs).
