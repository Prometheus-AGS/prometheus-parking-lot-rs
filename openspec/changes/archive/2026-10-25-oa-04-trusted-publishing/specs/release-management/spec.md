# Spec Delta

## MODIFIED Requirements

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
