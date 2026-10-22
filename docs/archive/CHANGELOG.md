# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Condvar (Condition Variable) implementation (2024-12-06)
  - `Condvar::new()` - Create a new condition variable
  - `wait(&mut MutexGuard)` - Block until notified
  - `wait_while(&mut MutexGuard, condition)` - Block while condition is true
  - `notify_one()` - Wake one waiting thread
  - `notify_all()` - Wake all waiting threads
  - 9 comprehensive unit tests
  - Full documentation with examples
  - Doc tests for all public methods

- Once and OnceCell primitives (2024-12-06)
  - `Once` from parking_lot for one-time initialization
  - `OnceCell` (re-export of `std::sync::OnceLock`) for lazy initialization
  - 10 comprehensive tests (4 Once + 6 OnceCell)
  - Full documentation with examples

- RwLock implementation (2024-12-06)
  - `RwLock<T>` with read() and write() methods
  - Support for multiple concurrent readers
  - Exclusive writer access
  - 10 unit tests covering concurrent access patterns
  - Full documentation with examples

- Mutex implementation (2024-12-06)
  - `Mutex<T>` with lock() and try_lock() methods
  - No poisoning on panic
  - 7 unit tests covering basic and concurrent scenarios
  - Full documentation with examples

### Changed
- InMemoryQueue refactored from VecDeque+sort to BinaryHeap (2024-12-07)
  - **415x performance improvement** for 10,000-item queues (725ms → 1.75ms)
  - O(n² log n) → O(n log n) total complexity for n enqueue operations
  - Introduced `PriorityTask<P>` wrapper implementing `Ord` for heap ordering
  - Priority comparison: highest priority first (Critical > High > Normal > Low)
  - FIFO ordering within same priority level via `created_at_ms` comparison
  - Added comprehensive unit tests: `test_prune_expired`, `test_empty_queue`

- Added benchmarks for parking_lot primitives (2024-12-07)
  - `bench_parking_lot_mutex_uncontended` - measures single-thread mutex performance
  - `bench_parking_lot_mutex_vs_std` - compares parking_lot vs std::sync::Mutex
  - `bench_atomic_operations` - measures AtomicU32 load, fetch_add, CAS operations
  - `bench_condvar_notify` - measures Condvar notify_one/notify_all performance
  - `bench_queue_with_mutex` - measures InMemoryQueue under Mutex protection

### Deprecated

### Removed

### Fixed

### Security

## [0.1.0] - 2024-12-06

### Added
- Initial project structure
- Cargo.toml configuration
- Living documentation framework
- Zero-warning, zero-error build system

---

*This changelog is maintained following [Keep a Changelog](https://keepachangelog.com/) principles.*
