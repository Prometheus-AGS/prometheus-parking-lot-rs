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
