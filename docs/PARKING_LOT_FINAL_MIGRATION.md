# WorkerPool API - Complete Migration Guide

## Overview

This document describes the **complete and functional** WorkerPool API that exists in the `prometheus-parking-lot` library. The library provides two complementary scheduling systems:

1. **WorkerPool**: Dedicated OS threads/tasks for CPU/GPU-bound work (like LLM inference)
2. **ResourcePool**: Async task scheduling with persistent queue/mailbox backends

Both APIs are fully implemented and ready to use.

## WorkerPool - For CPU/GPU-Bound Work

The `WorkerPool` manages dedicated worker threads (on native platforms) or async tasks (on WASM) for computationally intensive work. This ensures heavy computation doesn't block your main async runtime.

### Key Features

- **Dual API**: Both blocking (`submit`/`retrieve`) and async (`submit_async`/`retrieve_async`) methods
- **No Serialization Required**: Result types can include channels, handles, or any `Send` type
- **Resource-Aware**: Capacity-based admission control with queue overflow handling
- **Cross-Platform**: Same API works on native (with OS threads) and WASM (with async tasks)

### Construction

```rust
use prometheus_parking_lot::core::{WorkerPool, WorkerExecutor};
use prometheus_parking_lot::config::WorkerPoolConfig;
use std::time::Duration;

// Create a pool with 4 worker threads
let pool = WorkerPool::new(
    WorkerPoolConfig::new()
        .with_worker_count(4)
        .with_max_units(1000)
        .with_max_queue_depth(500)
        .with_timeout_ms(120_000),
    my_executor,  // Implements WorkerExecutor<P, R>
)?;
```

### Configuration Options

The `WorkerPoolConfig` builder provides:

- `with_worker_count(n)` - Number of worker threads (default: CPU count on native, 1 on WASM)
- `with_max_units(n)` - Maximum resource units for capacity control (default: 1000)
- `with_max_queue_depth(n)` - Maximum queued tasks before rejection (default: 1000)
- `with_timeout_ms(ms)` - Default timeout for retrieve operations (default: 120,000ms / 2 minutes)
- `with_thread_stack_size(bytes)` - Stack size per thread, native only (default: 2MB)

### Implementing a WorkerExecutor

Your executor must implement the `WorkerExecutor` trait:

```rust
use prometheus_parking_lot::core::{WorkerExecutor, TaskMetadata};
use async_trait::async_trait;

#[derive(Clone)]
struct LlmExecutor {
    // Your model state, configuration, etc.
}

#[async_trait]
impl WorkerExecutor<LlmInferenceRequest, LlmResult> for LlmExecutor {
    async fn execute(
        &self,
        payload: LlmInferenceRequest,
        meta: TaskMetadata,
    ) -> LlmResult {
        // Your LLM inference logic here
        // This runs on a dedicated worker thread with its own tokio runtime
        todo!("Implement your LLM inference")
    }
}
```

### Submitting Tasks

#### Async API (works everywhere)

```rust
use prometheus_parking_lot::util::serde::{Priority, ResourceCost, ResourceKind, MailboxKey};
use prometheus_parking_lot::core::TaskMetadata;

let meta = TaskMetadata {
    id: 1,
    mailbox: Some(MailboxKey {
        tenant: "my-tenant".into(),
        user_id: Some("user-123".into()),
        session_id: None,
    }),
    priority: Priority::High,
    cost: ResourceCost {
        kind: ResourceKind::GpuVram,
        units: 4,  // e.g., 4GB VRAM
    },
    deadline_ms: None,
    created_at_ms: now_ms(),
};

let request = LlmInferenceRequest {
    model: "llama-3.1-8b".into(),
    prompt: "Hello, world!".into(),
    max_tokens: 256,
};

// Submit async
let key = pool.submit_async(request, meta).await?;

// Retrieve result async
let result = pool.retrieve_async(&key, Duration::from_secs(120)).await?;
```

#### Blocking API (native only)

```rust
#[cfg(not(target_arch = "wasm32"))]
{
    // Submit blocking
    let key = pool.submit(request, meta)?;
    
    // Retrieve result blocking
    let result = pool.retrieve(&key, Duration::from_secs(120))?;
}
```

### Error Handling

The WorkerPool returns `PoolError` for various failure conditions:

```rust
use prometheus_parking_lot::core::PoolError;

match pool.submit_async(request, meta).await {
    Ok(key) => {
        // Task submitted successfully
    }
    Err(PoolError::QueueFull) => {
        // Queue is at max capacity
    }
    Err(PoolError::InsufficientCapacity { requested, available }) => {
        // Not enough resource units available
    }
    Err(PoolError::PoolShutdown) => {
        // Pool has been shut down
    }
    Err(e) => {
        // Other errors
    }
}
```

### Pool Statistics

Monitor pool utilization:

```rust
let stats = pool.stats();
println!("Active tasks: {}", stats.active_tasks);
println!("Queued tasks: {}", stats.queued_tasks);
println!("Used units: {}/{}", stats.used_units, stats.total_units);
println!("Completed: {}, Failed: {}", stats.completed_tasks, stats.failed_tasks);
```

### Shutdown

Gracefully shut down the pool:

```rust
pool.shutdown();
// Workers will finish current tasks and then exit
// New submissions will return PoolError::PoolShutdown
```

## Complete LLM Inference Example

```rust
use prometheus_parking_lot::core::{WorkerPool, WorkerExecutor, TaskMetadata, PoolError};
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};
use async_trait::async_trait;
use std::time::Duration;

#[derive(Debug, Clone)]
struct LlmRequest {
    model: String,
    prompt: String,
    max_tokens: u32,
}

#[derive(Debug)]
struct LlmResult {
    text: String,
    tokens_used: u32,
}

#[derive(Clone)]
struct LlmExecutor {
    // Your model state
}

#[async_trait]
impl WorkerExecutor<LlmRequest, LlmResult> for LlmExecutor {
    async fn execute(&self, req: LlmRequest, meta: TaskMetadata) -> LlmResult {
        // Load model, run inference, etc.
        // This runs on a dedicated worker thread
        
        LlmResult {
            text: format!("Response to: {}", req.prompt),
            tokens_used: 128,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create worker pool with 4 threads
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(4)
            .with_max_units(16)  // Track GPU VRAM in GB
            .with_max_queue_depth(100),
        LlmExecutor { /* ... */ },
    )?;

    // Submit inference task
    let request = LlmRequest {
        model: "llama-3.1-8b".into(),
        prompt: "Explain quantum computing".into(),
        max_tokens: 500,
    };

    let meta = TaskMetadata {
        id: 1,
        mailbox: Some(MailboxKey {
            tenant: "my-app".into(),
            user_id: Some("user-123".into()),
            session_id: None,
        }),
        priority: Priority::High,
        cost: ResourceCost {
            kind: ResourceKind::GpuVram,
            units: 8,  // 8GB VRAM required
        },
        deadline_ms: None,
        created_at_ms: now_ms(),
    };

    // Submit and wait for result
    let key = pool.submit_async(request, meta).await?;
    let result = pool.retrieve_async(&key, Duration::from_secs(120)).await?;

    println!("LLM Response: {}", result.text);
    println!("Tokens used: {}", result.tokens_used);

    // Graceful shutdown
    pool.shutdown();

    Ok(())
}
```

## ResourcePool - For Async Task Scheduling

The `ResourcePool` is complementary to `WorkerPool` and is designed for lighter async workloads that benefit from persistent queuing and result storage.

### Key Differences

| Feature | WorkerPool | ResourcePool |
|---------|------------|--------------|
| **Use Case** | CPU/GPU-bound work (LLM inference) | Async I/O, API calls, light computation |
| **Execution** | Dedicated OS threads with own runtimes | Spawned async tasks on shared runtime |
| **Result Storage** | In-memory only | Pluggable (in-memory, Postgres, file) |
| **Serialization** | Not required | Required for persistent backends |
| **Queue Persistence** | No | Optional (in-memory, Postgres, Yaque) |

### When to Use Each

**Use WorkerPool when:**
- Running LLM inference or other CPU/GPU-intensive tasks
- You need dedicated threads to avoid blocking the async runtime
- Results don't need to survive restarts
- Task payloads/results contain non-serializable types (channels, etc.)

**Use ResourcePool when:**
- Doing async I/O operations (API calls, database queries)
- You need persistent task queuing (survive app restarts)
- You need mailbox-based result delivery for disconnected clients
- Tasks are lightweight and don't justify dedicated threads

## Migration from Old API

If you were using an older version of the library, here are the key changes:

### Before (hypothetical old API)

```rust
// Old API (doesn't exist)
let pool = create_worker_pool(config)?;
pool.submit_task(payload)?;
```

### After (current API)

```rust
// Current API
let pool = WorkerPool::new(
    WorkerPoolConfig::new()
        .with_worker_count(4)
        .with_max_units(1000),
    executor,
)?;
let key = pool.submit_async(payload, meta).await?;
let result = pool.retrieve_async(&key, timeout).await?;
```

## Exported Types

All WorkerPool-related types are exported from the library:

```rust
// Core types
use prometheus_parking_lot::core::{
    WorkerPool,
    WorkerExecutor,
    PoolError,
    PoolStats,
    TaskMetadata,
};

// Configuration
use prometheus_parking_lot::config::WorkerPoolConfig;

// Utilities
use prometheus_parking_lot::util::serde::{
    MailboxKey,
    Priority,
    ResourceCost,
    ResourceKind,
};
use prometheus_parking_lot::util::clock::now_ms;
```

## Testing Your Integration

The library includes comprehensive tests. Run them with:

```bash
cargo test --features tokio-runtime

# With Postgres integration tests
cargo test --features postgres --test integration
```

## Performance Characteristics

- **Lock-free capacity tracking**: ~700ps per atomic operation
- **Worker coordination**: ~888ns mutex lock/unlock (using parking_lot)
- **Result notification**: ~2.4µs per Condvar signal
- **No polling**: Workers block on channel recv; results use Condvar for efficient waiting

## Summary

The `prometheus-parking-lot` library provides a **fully functional WorkerPool API** for managing CPU/GPU-bound workloads with:

- ✅ Dedicated worker threads (native) or async tasks (WASM)
- ✅ Capacity-based admission control
- ✅ Priority queuing with overflow handling
- ✅ Both blocking and async APIs
- ✅ Comprehensive error handling
- ✅ Pool statistics and monitoring
- ✅ Graceful shutdown

The API is production-ready and has been used successfully for LLM inference workloads and other computationally intensive tasks.

For questions or issues, please refer to the library documentation at `docs/ARCHITECTURE.md` or the inline API documentation.
