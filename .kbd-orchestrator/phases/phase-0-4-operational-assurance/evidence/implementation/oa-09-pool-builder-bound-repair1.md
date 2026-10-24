# OA09 constructor bound — facade native-discovery repair 1

Same cumulative final-boundary targeted core/facade repair batch; independent
review correction cycle remains1. Repaired source NOT_EXERCISED.

Read actual /tmp/oa04-boundary-729cc42/baseline/native-tests-discovery.stderr.txt.
Parent's real workspace/all-features/locked nextest native discovery compilation
exited101 with E0277: src/builders/pool_builder.rs83 passes Q to ResourcePool::new,
whose method requires Q: TaskQueue<P> at resource_pool.rs302. The generic public
build_pools where-clause did not propagate that requirement. Constructor queue
length accounting for already parked work is retained unchanged.

Synced src/builders/pool_builder.rs from the latest central formatted source.
The only change imports the existing crate::core::TaskQueue reexport and adds
Q: TaskQueue<P> to build_pools. No new API, reexport, backend, constructor or
other builder signature was changed. src/core/mod.rs already reexports the
same core scheduling contract at its root and resource_pool compatibility path.

Source inspection of build_pools call sites under src/tests/examples found the
four direct generic calls in tests/unit/builders_test.rs: configured name map,
resource limits, validation-before-factories, and embedded durable rejection.
Each queue factory returns InMemoryQueue<String>, the existing TaskQueue queue
implementation. Other config callers use build_pools_from_scheduler_config or
its hooks variant and require no propagation patch. No caller code changed.

Only targeted reads/searches, source copying/editing, diff generation and exact
SHA hashing ran. No tests, Cargo, discovery, compilation, Clippy, formatting,
syntax checks, scans, review or runtime mutations ran. No KBD/checkbox, commit,
push or external action. The immutable729cc425 verification checkout was not
touched. This source remains unverified until the driver's next authorized
integration check. The import/source map and candidate refresh are driver-owned;
this patch extends the same repair batch as oa-06-07-10-core-clippy-repair1.
