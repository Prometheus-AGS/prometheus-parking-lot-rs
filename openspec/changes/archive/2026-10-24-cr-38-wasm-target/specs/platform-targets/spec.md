## ADDED Requirements

### Requirement: Web target

The core and wasm crates SHALL build for wasm32-unknown-unknown, the wasm pool SHALL enforce capacity and execution budgets, and the core crate SHALL have no tokio in its wasm dependency graph.

#### Scenario: Wasm check

- **WHEN** CI builds the wasm crates and runs wasm-bindgen-test in a headless browser
- **THEN** the build passes, and smoke, capacity and budget tests pass

#### Scenario: Core graph

- **WHEN** the core dependency check runs
- **THEN** no tokio appears; a scratch branch adding tokio to core fails the check
