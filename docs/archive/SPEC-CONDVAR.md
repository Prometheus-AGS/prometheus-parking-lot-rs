# Condvar Implementation Specification

> **Feature:** Condition Variable (Condvar)  
> **Created:** 2024-12-06  
> **Status:** Phase 1 - SPECIFY  
> **Complexity:** Medium (2-3 hours)

---

## Problem Statement

We need a condition variable primitive that allows threads to:
1. Wait for a condition to become true while releasing a mutex
2. Be notified when the condition might have changed
3. Handle spurious wakeups correctly

**Use Cases:**
- Producer-consumer queues
- Thread pools waiting for work
- Signaling between threads
- Waiting for state changes

---

## Proposed Solution

Re-export `parking_lot::Condvar` with full API compatibility.

**Rationale:**
- Battle-tested implementation
- Zero-cost wrapper
- Compatible with our Mutex type
- Well-documented API

---

## Public API

```rust
/// A condition variable for thread synchronization.
///
/// Condition variables represent the ability to block a thread such that
/// it consumes no CPU time while waiting for an event to occur.
///
/// # Examples
///
/// ```
/// use prometheus_parking_lot::{Mutex, Condvar};
/// use std::sync::Arc;
/// use std::thread;
///
/// let pair = Arc::new((Mutex::new(false), Condvar::new()));
/// let pair2 = Arc::clone(&pair);
///
/// thread::spawn(move || {
///     let (lock, cvar) = &*pair2;
///     let mut started = lock.lock();
///     *started = true;
///     cvar.notify_one();
/// });
///
/// let (lock, cvar) = &*pair;
/// let mut started = lock.lock();
/// while !*started {
///     cvar.wait(&mut started);
/// }
/// ```
pub struct Condvar {
    inner: parking_lot::Condvar,
}

impl Condvar {
    /// Creates a new condition variable.
    pub const fn new() -> Self;
    
    /// Blocks the current thread until notified.
    pub fn wait<T>(&self, guard: &mut MutexGuard<'_, T>);
    
    /// Blocks until notified and condition is met.
    pub fn wait_while<T, F>(&self, guard: &mut MutexGuard<'_, T>, condition: F)
    where
        F: FnMut(&mut T) -> bool;
    
    /// Wakes up one blocked thread.
    pub fn notify_one(&self);
    
    /// Wakes up all blocked threads.
    pub fn notify_all(&self);
}
```

---

## Test Cases

### Unit Tests (7-10 tests)

1. **test_basic_wait_notify** - Thread waits and is notified
2. **test_notify_one_single_thread** - Only one thread wakes up
3. **test_notify_all_multiple_threads** - All threads wake up
4. **test_wait_while_predicate** - Wait with condition
5. **test_spurious_wakeup_handling** - Correctly handles spurious wakeups
6. **test_producer_consumer** - Classic pattern
7. **test_multiple_waiters** - Many threads waiting
8. **test_notify_before_wait** - Notify before waiting (edge case)
9. **test_condvar_with_mutex** - Integration with Mutex
10. **test_concurrent_notify** - Concurrent notify operations

---

## Implementation Plan

### Step 1: Create src/condvar.rs

```rust
//! Condition variable implementation.

use crate::MutexGuard;
use parking_lot;

/// A condition variable.
pub struct Condvar {
    inner: parking_lot::Condvar,
}

impl Condvar {
    /// Creates a new condition variable.
    #[inline]
    pub const fn new() -> Self {
        Self {
            inner: parking_lot::Condvar::new(),
        }
    }
    
    /// Blocks until notified.
    #[inline]
    pub fn wait<T>(&self, guard: &mut MutexGuard<'_, T>) {
        self.inner.wait(guard);
    }
    
    // ... rest of methods
}
```

### Step 2: Update src/lib.rs

Add:
```rust
mod condvar;
pub use condvar::Condvar;
```

### Step 3: Write Tests

In `src/condvar.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mutex;
    use std::sync::Arc;
    use std::thread;
    
    #[test]
    fn test_basic_wait_notify() {
        // Test implementation
    }
    
    // ... more tests
}
```

---

## Acceptance Criteria

- [ ] `src/condvar.rs` created with full implementation
- [ ] `src/lib.rs` updated with Condvar module and re-export
- [ ] All public items documented with examples
- [ ] 7-10 unit tests implemented
- [ ] All quality gates pass:
  - [ ] `cargo check`
  - [ ] `cargo clippy -- -D warnings` (0 warnings)
  - [ ] `cargo test` (all tests passing)
  - [ ] `cargo doc --no-deps` (no warnings)
  - [ ] `cargo fmt --check`
- [ ] `docs/CHANGELOG.md` updated
- [ ] `docs/TODO.md` updated

---

## Risk Assessment

**Risk Level:** Low

**Reasons:**
- Leveraging proven parking_lot implementation
- API well-defined from std::sync
- Pattern widely understood

**Mitigation:**
- Follow parking_lot docs closely
- Test with multiple threads
- Include spurious wakeup handling

---

## Dependencies

- `parking_lot = "0.12"` ✅ (already in Cargo.toml)
- Requires: Mutex and MutexGuard ✅ (already implemented)

---

## Files

### To Create:
- `src/condvar.rs`

### To Modify:
- `src/lib.rs`
- `docs/CHANGELOG.md`
- `docs/TODO.md`

---

*Specification complete. Ready for Phase 2: PLAN.*
