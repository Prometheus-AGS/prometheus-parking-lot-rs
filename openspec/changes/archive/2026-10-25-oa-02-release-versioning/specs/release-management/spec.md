# Spec Delta

## ADDED Requirements

### Requirement: Seven-crate version policy

The release SHALL use a documented seven-crate version policy and deterministic changelog preparation, excluding unpublished FFI.

#### Scenario: Choose and automate seven-crate versioning

- **WHEN** the supported operation encounters the named condition in OA-02
- **THEN** The adopted ADR chooses lockstep 0.4.0 (proposal) or an operator-amended policy; a dry run yields consistent internal dependency versions and changelog entries for exactly seven publishable crates, excluding FFI; a second run is unchanged.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Inject a mismatched internal dependency/version in the scratch copy; the release-preparation check must fail.
