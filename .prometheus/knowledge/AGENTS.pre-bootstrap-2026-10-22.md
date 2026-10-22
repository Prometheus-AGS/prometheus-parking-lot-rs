# prometheus-parking-lot Development Guidelines

Auto-generated from all feature plans. Last updated: 2025-12-06

## Active Technologies
- Rust (edition 2021, stable 1.78+ preferred) + async-trait, serde/serde_json, tracing, thiserror/anyhow (app), optional Postgres/pgmq-style and Yaque backends, tokio for native adapter (001-parking-system-build)
- Pluggable queue/mailbox backends: in-memory, file/embedded (Yaque), Postgres/pgmq-style; audit storage aligned with chosen backend (001-parking-system-build)

- (001-parking-system-build)

## Project Structure

```text
backend/
frontend/
tests/
```

## Commands

# Add commands for 

## Code Style

: Follow standard conventions

## Recent Changes
- 001-parking-system-build: Added Rust (edition 2021, stable 1.78+ preferred) + async-trait, serde/serde_json, tracing, thiserror/anyhow (app), optional Postgres/pgmq-style and Yaque backends, tokio for native adapter

- 001-parking-system-build: Added

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
