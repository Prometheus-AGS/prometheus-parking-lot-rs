# durable-queue-pgmq Specification

## Purpose
Map pgmq SQL operations and visibility timeouts to the shared lease-queue contract without a pgmq client-crate dependency.

## Requirements

### Requirement: pgmq-backed queue

The pgmq backend SHALL satisfy the queue conformance suite using visibility timeouts as leases and per-priority queues.

#### Scenario: Priority order

- **WHEN** tasks of three priorities are enqueued
- **THEN** higher-priority tasks are leased first
