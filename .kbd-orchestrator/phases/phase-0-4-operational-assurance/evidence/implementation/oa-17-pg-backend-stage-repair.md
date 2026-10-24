# OA17 benchmark observed Clippy stage repair

Source-only; repaired benchmark NOT_EXERCISED. Parent's actual
/tmp/oa04-observed-repair6-clippy.log records exit101 and three bench diagnostics:
doc_markdown for pg_measure.py, sample too_many_lines106/100, and unused must-use
Enqueued. Other owners' diagnostics in that log are outside this source delta.
The log and complete benchmark were read; no tool command was rerun.

Synced current central formatted benches/pg_backend.rs before editing. Changes:

- Backtick the existing pg_measure.py documentation reference.
- Bind the queue enqueue receipt as _enqueued, preserving its awaited database
  operation, error check and all lease/payload/ack observations.
- Extract only the existing database connection-startup loop into
  connect_database() -> PgPool. It still reads the same required environment
  variable once, uses the same60-second deadline, same PgPool::connect, same
 250ms retry sleep and exact panic condition/message. sample awaits this helper
  at the original position before max-connection observation and migrations.
  This removes the startup stage's lines from sample without a lint allow or
  changing any workload operation, limit, result format or measured phase.

The previous Clone repair remains intact. The phase function and all Instant
boundaries are untouched: warm-up still precedes the measured phase; the
explicit slow-control sleep remains inside each timed client cycle; duration,
latency/checksum and full-process RSS scope are unchanged by the source delta.
Migrations, hot-key seed, shutdown/drain, queue-depth/result checks, JSON output
and cleanup/observer ordering remain at their original positions. The same
source remains transplantable into baseline0.3 and candidate; no package or
source-map metadata was edited.

Only source/log reads, source copying/editing, exact SHA hashing and patch
creation ran. No tests, Cargo, builds, Clippy, formatting, syntax checks,
review, scans, runtime, KBD/checkbox, commits or external actions. Central and
source maps were untouched. No unrelated additions. All three changes trace
to observed compiler/lint diagnostics. Actual lint acceptance and measurements
remain unverified. This delta postdates the prior fixture-batch5 review and
must be included in the parent's authorized reviewer follow-up and candidate/
source-map refresh rather than represented as covered by that prior review.
