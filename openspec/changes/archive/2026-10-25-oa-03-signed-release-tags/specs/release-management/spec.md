# Spec Delta

## ADDED Requirements

### Requirement: Verified release tag identity

An authorized release tag SHALL be annotated, signed by an approved identity and bound to the exact release candidate; unsigned, wrong-identity and wrong-candidate tags SHALL be rejected.

#### Scenario: Verify operator-signed release tags

- **WHEN** the supported operation encounters the named condition in OA-03
- **THEN** A scratch annotated tag signed by an operator-authorized identity verifies at the frozen candidate; an unsigned, wrong-identity or wrong-commit tag is rejected. Real v0.4.0 tag creation remains an approved publication step.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Unsigned tag and a signed tag pointing at a different commit each fail candidate authorization.
