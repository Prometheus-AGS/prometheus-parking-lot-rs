## ADDED Requirements

### Requirement: Mobile and desktop reach

The FFI crate SHALL build for Android and iOS targets, and a Tauri example SHALL run the embedded queue end to end.

#### Scenario: Mobile builds

- **WHEN** CI builds aarch64-linux-android and aarch64-apple-ios(-sim)
- **THEN** both succeed and Swift/Kotlin bindings are generated

#### Scenario: Desktop example

- **WHEN** the Tauri example submits and retrieves a task across an app restart
- **THEN** the result is retrieved after restart
