# crate-structure Specification

## Purpose
Separate the runtime-free scheduling core from runtime, storage and platform integrations while retaining the public facade.

## Requirements

### Requirement: Layered workspace

The core crate SHALL compile for wasm32-unknown-unknown with no default features and SHALL NOT depend on any IO, database, network or runtime crate; the facade SHALL preserve the published crate name.

#### Scenario: Core dependency boundary

- **WHEN** `cargo tree -p prometheus-parking-lot-core -e normal` is inspected
- **THEN** no tokio, sqlx, sea-orm, iggy, redb or reqwest appears

#### Scenario: Facade compatibility

- **WHEN** a downstream crate depends on prometheus_parking_lot with default features
- **THEN** it compiles and reaches the core types through the facade
