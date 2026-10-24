# release-management Specification

## Purpose
Record the approved publication boundary and actual 0.3.0 release, with deferred automation and signing controls made explicit.

## Requirements

### Requirement: Reproducible, approved release

Release 0.3.0 SHALL require an adopted boundary PASS and operator approval. All seven publishable crates SHALL be installable from crates.io at 0.3.0; FFI remains unpublished. The repository SHALL carry an annotated v0.3.0 tag pointing at the released commit.

#### Scenario: Install from crates.io

- **WHEN** a fresh project runs `cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded` and builds
- **THEN** it resolves only 0.3.0 workspace crates from crates.io and compiles

#### Scenario: Tag and release

- **WHEN** the operator completes the approved 0.3.0 publication
- **THEN** the annotated v0.3.0 tag resolves to the released commit and the GitHub release lists the changelog for 0.3.0; signature verification is deferred to #87

#### Scenario: Gate enforced

- **WHEN** a release is proposed without a recorded adopted boundary PASS result or operator approval
- **THEN** publication remains unauthorized; enforcement by an OIDC release workflow is deferred to #85

#### Scenario: Deferred automation disposition

- **WHEN** the operator closes this phase under decision `phase-correctness-and-durability-close-deferrals-20261024`
- **THEN** versioning automation, trusted publishing and workflow tasks remain deferred to #85/#86, and tag signing remains #87; administrative task closure does not certify their implementation
