# build-baseline Specification

## Purpose
Keep every workspace target buildable on the pinned toolchain and declared MSRV with reproducible dependency resolution.

## Requirements

### Requirement: Reproducible build on the pinned toolchain

The crate SHALL build, lint and test with `--locked` on Rust 1.99.1 and on the declared MSRV 1.95, for all targets and all features.

#### Scenario: All targets compile

- **WHEN** `cargo check --workspace --all-targets --all-features --locked` runs on 1.99.1 and on 1.95
- **THEN** both exit 0

#### Scenario: Lint and format clean

- **WHEN** `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` and `cargo fmt --all -- --check` run
- **THEN** both exit 0
