## ADDED Requirements

### Requirement: Reproducible, approved release

Release 0.3.0 SHALL be published only after the boundary packet passes and the operator approves, every crate SHALL be installable from crates.io at 0.3.0, and the repository SHALL carry a signed v0.3.0 tag pointing at the released commit.

#### Scenario: Install from crates.io

- **WHEN** a fresh project runs `cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded` and builds
- **THEN** it resolves only 0.3.0 workspace crates from crates.io and compiles

#### Scenario: Tag and release

- **WHEN** the release workflow completes
- **THEN** `git tag -v v0.3.0` verifies and the GitHub release lists the changelog for 0.3.0

#### Scenario: Gate enforced

- **WHEN** the publish workflow is started without a recorded boundary PASS receipt
- **THEN** it refuses to publish
