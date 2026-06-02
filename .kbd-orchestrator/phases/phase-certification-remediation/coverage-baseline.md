# Coverage Baseline — CR-04

**Tool:** `cargo llvm-cov 0.8.7` · **Date:** 2026-06-02 · **Suite:** all targets, green, no hang.

## Totals
| Metric | Coverage |
|--------|----------|
| Lines | **55.73%** (687/1552 missed) |
| Regions | 61.14% |
| Functions | 49.43% |

## Per-module (lines)
| Module | Line cov | Note |
|--------|----------|------|
| `core/worker_pool/native.rs` | **92.67%** | well covered (incl. CR-01/CR-02) |
| `core/worker_pool.rs` | 94.20% | |
| `infra/queue/memory.rs` | 97.52% | |
| `core/resource_pool.rs` | 61.33% | scheduler core |
| `infra/mailbox/memory.rs` | 46.34% | improve in CR-06 |
| `infra/mailbox/postgres.rs` | **0%** | needs live DB |
| `infra/mailbox/yaque.rs` | **0%** | needs filesystem harness |
| `infra/queue/postgres.rs` | **0%** | needs live DB |
| `infra/queue/yaque.rs` | **0%** | needs filesystem harness |

## Reading
The **core scheduling logic is well-tested**; the headline 55.73% is dragged down almost
entirely by the **persistent backends (postgres/yaque) at 0%**, which require external
infrastructure not wired into the test harness. Excluding those four files, the in-process
code is ~80%+.

## Follow-ups (not blockers)
- CR-06: raise `infra/mailbox/memory.rs` coverage (TTL/eviction/redelivery tests).
- CR-10: CI job runs `cargo llvm-cov` + a **suite-level wall-clock watchdog** (the per-test
  `with_timeout` guard exists in `worker_pool_test.rs`; CI should also cap total runtime so a
  future hang fails loudly instead of blocking).
- Consider a feature-gated integration harness (testcontainers / tempdir) for postgres/yaque
  so those backends can be covered in CI.
