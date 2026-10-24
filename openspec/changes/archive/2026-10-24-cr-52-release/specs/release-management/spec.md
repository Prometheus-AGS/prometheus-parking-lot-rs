## ADDED Requirements

### Requirement: Reproducible, approved release

Release 0.3.0 SHALL be published only after the adopted boundary packet passes and the operator approves. All seven publishable crates SHALL be installable from crates.io at 0.3.0; the FFI crate remains unpublished. The repository SHALL carry an annotated v0.3.0 tag pointing at the released commit. The actual tag is unsigned. Versioning automation, trusted publishing, the gated release workflow and signed tags carry to 0.4.0 under issues #85–#87, per the operator's 2026-10-24 closure decision `phase-correctness-and-durability-close-deferrals-20261024`; their administrative closure does not certify their implementation.

#### Scenario: Install from crates.io

- **WHEN** a fresh project runs `cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded` and builds
- **THEN** it resolves only 0.3.0 workspace crates from crates.io and compiles

#### Scenario: Tag and release

- **WHEN** the operator completes the approved 0.3.0 publication
- **THEN** the annotated v0.3.0 tag resolves to the released commit and the GitHub release lists the changelog for 0.3.0; signature verification is deferred to #87

#### Scenario: Gate enforced

- **WHEN** a release is proposed without a recorded adopted boundary PASS result or operator approval
- **THEN** publication remains unauthorized; enforcement by an OIDC release workflow is deferred to #85
