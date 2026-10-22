## ADDED Requirements

### Requirement: Gap-free resume

A client that disconnects mid-stream SHALL be able to resume and receive the remaining tokens with no gap or duplicate.

#### Scenario: Reconnect mid-stream

- **WHEN** a client disconnects after token 50 of 200 and reconnects
- **THEN** it receives tokens 51-200 exactly once
