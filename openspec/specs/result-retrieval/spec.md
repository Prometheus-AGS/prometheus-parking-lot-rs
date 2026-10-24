# result-retrieval Specification

## Purpose
Let callers await or synchronously retrieve results with bounded retention without allocating a blocking thread for each async waiter.

## Requirements

### Requirement: Non-blocking retrieval

Waiting for a result asynchronously SHALL NOT occupy a blocking thread, and unretrieved results SHALL expire after their TTL.

#### Scenario: Many waiters

- **WHEN** a runtime built with max_blocking_threads(1) has that thread occupied, and 10,000 concurrent retrieve_async calls wait for results that are then delivered
- **THEN** all 10,000 calls resolve within the test timeout

#### Scenario: TTL expiry

- **WHEN** a result is never retrieved
- **THEN** it is evicted after its TTL and a later retrieve reports expired
