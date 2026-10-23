# Benchmarking Guide

The benchmark suites, how to run them, where the results go, how they are made
reproducible, and the CI regression policy are documented in
[`benches/README.md`](benches/README.md). Measured numbers, with the machine,
toolchain and command they came from, are in
[`docs/ARCHITECTURE.md`, section 7](docs/ARCHITECTURE.md#7-performance-characteristics).

Quick start:

```bash
cargo bench --all-features --locked \
  --bench components --bench throughput --bench latency --bench utilization -- --noplot
```

When you quote a number, quote the machine, the load average and the command with it:
the suites write all three into `target/bench-report/*.json`, and the same code moves
by up to about 20% between back-to-back runs on a loaded machine.

## Profiling

```bash
# Flamegraph of one benchmark (requires cargo-flamegraph).
cargo flamegraph --bench throughput --all-features -- --bench throughput/resource_pool/unit

# Iterate one benchmark for a fixed time without analysis (for an external profiler).
cargo bench --all-features --bench components -- --profile-time 10 dispatcher/park_wake
```
