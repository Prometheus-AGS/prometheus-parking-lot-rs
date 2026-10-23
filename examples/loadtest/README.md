# Load Test Harness (Manual)

- Use `examples/configs/in-memory.json` as a baseline. `postgres.json` and `redb.json` run their pool durably (cr-49), so a load test on them measures the durable path (persist before accept, ack after delivery).
- Write a small driver (e.g., in Rust or a script) that submits 10k tasks with mixed priorities and deadlines, then polls status/mailbox.
- Measure:
  - 0 over-capacity starts (SC-001)
  - 95% queued tasks start within 1s after capacity frees (SC-002)
  - 99% mailbox retrieval after restart/disconnect (SC-003)
  - Audit retrieval ≤2s over 24h event volume (SC-004)
- Run with tracing enabled to observe overload and expiry paths.

Example outline (pseudo):

```
for i in 0..10000 {
  submit(pool="llm_inference", priority=if i%10==0 { "high" } else { "normal" }, deadline_ms=now+30000)
}
# Wait for processing, then poll status/mailbox to collect metrics.
```
