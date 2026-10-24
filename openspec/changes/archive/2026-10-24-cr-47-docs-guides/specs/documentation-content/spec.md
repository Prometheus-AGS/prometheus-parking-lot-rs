## ADDED Requirements

### Requirement: Working examples per scenario

Every guide code sample SHALL be compiled in CI from a real source file, and each scenario SHALL state when to use it and the value it delivers.

#### Scenario: Examples compile

- **WHEN** the docs example job runs
- **THEN** every referenced example builds and its smoke run passes
