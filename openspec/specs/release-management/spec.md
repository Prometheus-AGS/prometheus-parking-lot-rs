# release-management Specification

## Purpose
Record the approved publication boundary and actual 0.3.0 release, with deferred automation and signing controls made explicit.

## Requirements

### Requirement: Reproducible, approved release

Release 0.4.0 SHALL require a candidate-bound boundary PASS and separate operator publication approval. Seven publishable crates SHALL be installable at the version-policy-approved release; FFI remains unpublished. Publication SHALL use trusted publishing without a long-lived-token fallback and SHALL require a verified operator-signed release tag.

#### Scenario: Install from crates.io

- **WHEN** a fresh project installs the approved release with postgres and embedded features
- **THEN** every internal workspace dependency resolves to its approved published version and compiles

#### Scenario: Tag and release

- **WHEN** the operator completes the approved 0.4.0 publication
- **THEN** the signed annotated v0.4.0 tag verifies against the approved identity and exact released candidate, and the GitHub release lists the generated changelog

#### Scenario: Gate enforced

- **WHEN** a release is proposed without a current candidate-bound PASS receipt or separate operator approval
- **THEN** no publication occurs

#### Scenario: Deferred automation disposition

- **WHEN** the earlier 0.3.0 deferral decision is read during 0.4.0 planning
- **THEN** it is historical disposition only, and cannot satisfy the implemented automation, signing or publication evidence for 0.4.0

#### Scenario: Wrong candidate or missing environment approval

- **WHEN** the tag, receipt or approval environment does not authorize the exact candidate and seven-crate release set
- **THEN** the workflow rejects publication and never falls back to a long-lived token

### Requirement: Seven-crate version policy

The release SHALL use a documented seven-crate version policy and deterministic changelog preparation, excluding unpublished FFI.

#### Scenario: Choose and automate seven-crate versioning

- **WHEN** the supported operation encounters the named condition in OA-02
- **THEN** The adopted ADR chooses lockstep 0.4.0 (proposal) or an operator-amended policy; a dry run yields consistent internal dependency versions and changelog entries for exactly seven publishable crates, excluding FFI; a second run is unchanged.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Inject a mismatched internal dependency/version in the scratch copy; the release-preparation check must fail.

### Requirement: Verified release tag identity

An authorized release tag SHALL be annotated, signed by an approved identity and bound to the exact release candidate; unsigned, wrong-identity and wrong-candidate tags SHALL be rejected.

#### Scenario: Verify operator-signed release tags

- **WHEN** the supported operation encounters the named condition in OA-03
- **THEN** A scratch annotated tag signed by an operator-authorized identity verifies at the frozen candidate; an unsigned, wrong-identity or wrong-commit tag is rejected. Real v0.4.0 tag creation remains an approved publication step.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Unsigned tag and a signed tag pointing at a different commit each fail candidate authorization.
