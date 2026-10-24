## ADDED Requirements

### Requirement: WASM parity

The WASM pool SHALL enforce vector admission and emit the same lifecycle events as the native pools.

#### Scenario: Parity tests

- **WHEN** wasm-bindgen-test runs the shared admission and event-ordering cases
- **THEN** all pass
