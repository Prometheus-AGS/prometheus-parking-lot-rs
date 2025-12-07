# Parking Lot Scheduler Benchmarks

Comprehensive performance benchmarks for the `prometheus_parking_lot` scheduler.

## Running Benchmarks

### Run all benchmarks
```bash
cargo bench
```

### Run specific benchmark group
```bash
cargo bench --bench queue_bench -- queue_enqueue_dequeue
cargo bench --bench queue_bench -- mailbox_deliver
cargo bench --bench queue_bench -- pool_submit
```

### Quick test (no actual benchmarking)
```bash
cargo bench --bench queue_bench -- --test
```

## Benchmark Categories

### 1. Queue Benchmarks (`queue_benches`)

#### `queue_enqueue_dequeue`
- **What it measures**: Raw queue performance for enqueue and dequeue operations
- **Sizes tested**: 100, 1,000, 10,000 tasks
- **Key metric**: Throughput (operations/second)
- **Why it matters**: Queue is the core data structure for parking tasks

#### `queue_priority_sorting`
- **What it measures**: Performance of priority-based task ordering
- **Priorities**: Critical, High, Normal, Low (mixed in queue)
- **Sizes tested**: 100, 1,000, 5,000 tasks
- **Key metric**: Time to enqueue and sort all tasks
- **Why it matters**: Priority ordering is critical for fair scheduling

#### `queue_prune_expired`
- **What it measures**: Performance of removing expired tasks
- **Scenario**: 50% expired tasks mixed with valid tasks
- **Sizes tested**: 100, 1,000, 5,000 tasks
- **Key metric**: Time to identify and remove expired tasks
- **Why it matters**: Deadline enforcement prevents resource waste

### 2. Mailbox Benchmarks (`mailbox_benches`)

#### `mailbox_deliver`
- **What it measures**: Performance of delivering results to mailbox
- **Sizes tested**: 100, 1,000, 10,000 messages
- **Key metric**: Throughput (deliveries/second)
- **Why it matters**: Mailbox is how results reach disconnected clients

#### `mailbox_fetch`
- **What it measures**: Performance of retrieving messages from mailbox
- **Scenario**: Pre-populated mailbox with varying sizes
- **Sizes tested**: 100, 1,000, 5,000 messages
- **Key metric**: Time to fetch all messages for a mailbox key
- **Why it matters**: Clients need fast access to their results

### 3. ResourcePool Benchmarks (`pool_benches`)

#### `pool_submit_immediate`
- **What it measures**: Task submission when capacity is available
- **Scenario**: Submit tasks that fit within pool capacity
- **Capacities tested**: 10, 50, 100 units
- **Key metric**: Submission latency
- **Why it matters**: Low latency critical for immediate task execution

#### `pool_submit_with_queueing`
- **What it measures**: Task submission when capacity is exhausted
- **Scenario**: Submit more tasks than pool capacity (forces queueing)
- **Pool capacity**: 10 units (fixed)
- **Task counts**: 50, 100, 200 tasks
- **Key metric**: Throughput (submissions/second)
- **Why it matters**: Queue management overhead impacts overall throughput

#### `pool_mixed_priorities`
- **What it measures**: Scheduling performance with mixed priority tasks
- **Scenario**: 100 tasks with varied priorities (Critical, High, Normal, Low)
- **Distribution**: Realistic mix simulating production workload
- **Key metric**: Total scheduling time
- **Why it matters**: Priority scheduling must not significantly degrade performance

#### `pool_deadline_checking`
- **What it measures**: Performance of deadline enforcement
- **Scenario**: Submit 50 tasks with expired deadlines
- **Key metric**: Rejection latency
- **Why it matters**: Fast rejection prevents wasted resources

### 4. End-to-End Scenarios (`scenario_benches`)

#### `end_to_end_scenario/realistic_workload`
- **What it measures**: Complete parking lot algorithm under realistic conditions
- **Scenario**: 
  - 150 tasks submitted
  - Mixed priorities (20% Critical, 30% High, 30% Normal, 20% Low)
  - 10% have deadlines
  - Pool capacity: 25 units
  - Queue capacity: 500 tasks
- **Workflow**:
  1. Tasks submitted (some immediate, some queued)
  2. Tasks execute and complete
  3. Queued tasks wake up as capacity frees
  4. Results delivered to mailbox
- **Key metric**: Total scenario completion time
- **Why it matters**: Best representation of production performance

## Performance Targets

Based on typical AI agent workloads:

| Operation | Target | Why |
|-----------|--------|-----|
| Queue enqueue/dequeue | > 1M ops/sec | Core operations must be fast |
| Priority sorting | < 1ms for 1K tasks | Scheduling decisions must be quick |
| Task submission (immediate) | < 100μs | Low latency for responsive agents |
| Task submission (queued) | < 500μs | Acceptable overhead for parking |
| Mailbox delivery | > 100K msgs/sec | High throughput for concurrent tasks |

## Interpreting Results

### Good Performance Indicators
- ✅ Queue operations scale linearly with task count
- ✅ Priority sorting overhead < 10% vs FIFO queue
- ✅ Submission latency consistent regardless of queue depth
- ✅ Wake-up mechanism doesn't add significant overhead

### Performance Red Flags
- ⚠️ Queue operations slow down with size (> O(n log n))
- ⚠️ Submission latency increases with queue depth
- ⚠️ Priority sorting takes > 1ms for moderate workloads
- ⚠️ Memory usage grows unbounded

## Hardware Considerations

Benchmark results vary by:
- **CPU**: Single-threaded performance matters for queue operations
- **Memory**: Large queues need sufficient RAM
- **OS**: Thread scheduling affects async performance

Record your hardware specs when benchmarking:
```bash
# Linux
lscpu | grep "Model name"
free -h

# macOS
sysctl -n machdep.cpu.brand_string
sysctl -n hw.memsize | awk '{print $0/1024/1024/1024 " GB"}'
```

## Continuous Performance Testing

Run benchmarks regularly to catch performance regressions:

```bash
# Baseline
cargo bench --bench queue_bench -- --save-baseline main

# After changes
cargo bench --bench queue_bench -- --baseline main
```

## Future Benchmarks

Planned additions:
- [ ] Postgres queue backend benchmarks
- [ ] Yaque file queue benchmarks
- [ ] Multi-threaded contention scenarios
- [ ] Memory usage profiling
- [ ] Stress tests (millions of tasks)
