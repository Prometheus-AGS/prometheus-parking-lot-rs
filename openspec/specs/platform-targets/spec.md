# platform-targets Specification

## Purpose
Define the supported WASM, mobile and desktop surfaces and the checks that establish their documented capabilities.

## Requirements

### Requirement: Web target

The core and wasm crates SHALL build for wasm32-unknown-unknown, the wasm pool SHALL enforce capacity and execution budgets, and the core crate SHALL have no tokio in its wasm dependency graph.

#### Scenario: Wasm check

- **WHEN** CI builds the wasm crates and runs wasm-bindgen-test in a headless browser
- **THEN** the build passes, and smoke, capacity and budget tests pass

#### Scenario: Core graph

- **WHEN** the core dependency check runs
- **THEN** no tokio appears; a scratch branch adding tokio to core fails the check

### Requirement: Mobile and desktop reach

The FFI crate SHALL build for Android and iOS targets, and a Tauri example SHALL run the embedded queue end to end.

#### Scenario: Mobile builds

- **WHEN** CI builds aarch64-linux-android and aarch64-apple-ios(-sim)
- **THEN** both succeed and Swift/Kotlin bindings are generated

#### Scenario: Desktop example

- **WHEN** the Tauri example submits and retrieves a task across an app restart
- **THEN** the result is retrieved after restart

### Requirement: WASM parity

The WASM pool SHALL enforce vector admission and emit the same lifecycle events as the native pools.

#### Scenario: Parity tests

- **WHEN** wasm-bindgen-test runs the shared admission and event-ordering cases
- **THEN** all pass
