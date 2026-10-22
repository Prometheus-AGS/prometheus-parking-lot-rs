---
title: Worker pools
description: Thread and worker pools, work stealing from Cilk to Tokio, and why the library keeps admission separate from execution.
---

# Worker pools

Admission decides *whether* a task may start. A worker pool decides *where and
on which thread* it runs. This chapter covers worker pools and work stealing,
and explains how the library combines them with its parking lot.

## The idea

Creating a thread per task is expensive and unbounded. A **worker pool** keeps a
fixed set of long-lived workers and feeds them tasks through a queue. The pool
bounds concurrency by the number of workers, amortises thread start-up, and
gives one place to observe and shut down execution.

A pool of N workers limits how many tasks run at once. It does **not** limit how
much of a resource those tasks use. Four workers can run four tasks that each
need 10 GB of GPU memory on a 24 GB card. That is why this library treats
admission (units) and execution (workers) as two separate limits.

```mermaid
flowchart LR
  subgraph admission["Admission: units"]
    Q["Parking lot<br/>(priority queue)"] --> G{"cost fits<br/>free units?"}
  end
  subgraph execution["Execution: workers"]
    W1["Worker 1"]
    W2["Worker 2"]
    W3["Worker 3"]
  end
  G -->|yes| W1
  G -->|yes| W2
  G -->|yes| W3
  G -->|no| Q
```

## History

### Work stealing (Blumofe and Leiserson; Cilk)

When workers each keep their own queue of tasks, some run dry while others are
overloaded. **Work stealing** fixes this: an idle worker takes work from the
queue of a busy one. Robert Blumofe and Charles Leiserson gave a randomised
work-stealing scheduler for fully strict multithreaded computations with
proven bounds, presented at FOCS in 1994 and published in the
Journal of the ACM in 1999, with bounds on time, space and communication
([Blumofe and Leiserson, 1994](https://doi.org/10.1109/SFCS.1994.365680);
[Blumofe and Leiserson, 1999](https://doi.org/10.1145/324133.324234)).
The Cilk runtime system (Blumofe, Joerg, Kuszmaul, Leiserson, Randall and Zhou,
PPoPP 1995) put a work-stealing scheduler into a practical multithreaded
language runtime
([Blumofe et al., 1995](https://doi.org/10.1145/209936.209958)).

Doug Lea's fork/join framework for Java (2000) brought the same design to a
mainstream library: per-worker double-ended queues, with owners taking from one
end and thieves from the other
([Lea, 2000](https://doi.org/10.1145/337449.337465)).

### Tokio's scheduler (2019)

Tokio, the async runtime this library's native pools use, rewrote its
multi-threaded scheduler in 2019. Carl Lerche's post "Making the Tokio scheduler
10x faster" (13 October 2019) describes the design: "Each processor maintains
its own run queue"; idle processors steal from busy ones; and a "next task"
slot, which "the processor will always check ... before checking the run
queue", so a task woken by a message runs right after its sender while the data
is still in cache. The post credits the Go scheduler for its approach to
throttling wake-ups
([Lerche, 2019](https://tokio.rs/blog/2019-10-scheduler)).

### Why GPU and CPU-heavy work avoids the async scheduler

A work-stealing async runtime assumes tasks yield often. A task that runs a
long, blocking computation (model inference on the CPU, or a blocking call into
a GPU library) holds a runtime thread and delays every other task queued on it.
The usual remedy is to run such work on dedicated threads, outside the async
scheduler, and hand results back through a channel.

## How this library applies it

### What exists on `main`

- **`ResourcePool`** does not own threads. It hands each admitted task to a
  `Spawn` implementation (`TokioSpawner` on native), so execution runs on the
  application's Tokio runtime with its work-stealing scheduler. Every execution
  is bounded by `tokio::time::timeout` using the task's deadline or the pool's
  `default_timeout`; when the budget elapses, the executor future is dropped,
  which cancels it at its next await point.
- **The native `WorkerPool`** spawns `worker_count` dedicated OS threads. Each
  thread runs its own single-threaded Tokio runtime and blocks on its task, so
  CPU or GPU-bound work does not stall the application's async runtime.
  Workers take tasks from one shared bounded FIFO channel (crossbeam). There is
  no work stealing between workers, and none is needed: a single shared queue
  already keeps every idle worker busy.
- **What the worker pools do not do yet.** The native `WorkerPool` counts
  units but does not gate admission on `max_units`, and it ignores priority
  (the channel is FIFO). The WASM pool sizes its semaphore by `worker_count`,
  not by units. An executor panic is not caught.

### Planned

- `cr-19-dispatcher-semaphore`: a single dispatcher in front of every pool,
  admitting by a unit-weighted semaphore in priority order, and feeding the
  native workers instead of letting submitters write to their channel
  directly.
- `cr-21-panic-containment`: `catch_unwind` around execution, capacity released
  by a guard even during a panic, and dead workers replaced.
- `cr-29-shutdown-cancellation`: shutdown policies (drain, drain then cancel,
  cancel now) applied by both pools.
- `cr-53-wasm-parity`: the WASM pool follows the dispatcher changes.

## Sources

1. Robert D. Blumofe, Charles E. Leiserson. "Scheduling Multithreaded Computations by Work Stealing." *35th Annual Symposium on Foundations of Computer Science (FOCS)*, 1994, pp. 356 to 368. [doi:10.1109/SFCS.1994.365680](https://doi.org/10.1109/SFCS.1994.365680)
2. Robert D. Blumofe, Charles E. Leiserson. "Scheduling Multithreaded Computations by Work Stealing." *Journal of the ACM* 46(5), 1999, pp. 720 to 748. [doi:10.1145/324133.324234](https://doi.org/10.1145/324133.324234)
3. Robert D. Blumofe, Christopher F. Joerg, Bradley C. Kuszmaul, Charles E. Leiserson, Keith H. Randall, Yuli Zhou. "Cilk: An Efficient Multithreaded Runtime System." *PPoPP 1995*, pp. 207 to 216. [doi:10.1145/209936.209958](https://doi.org/10.1145/209936.209958)
4. Doug Lea. "A Java Fork/Join Framework." *Proceedings of the ACM 2000 Conference on Java Grande*, 2000. [doi:10.1145/337449.337465](https://doi.org/10.1145/337449.337465)
5. Carl Lerche. "Making the Tokio scheduler 10x faster." Tokio blog, 13 October 2019. [tokio.rs](https://tokio.rs/blog/2019-10-scheduler)
