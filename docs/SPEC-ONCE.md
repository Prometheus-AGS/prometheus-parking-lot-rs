# Specification: Once and OnceCell Implementation

> **Feature:** Once and OnceCell synchronization primitives  
> **Created:** 2024-12-06  
> **Status:** Phase 1 - SPECIFY  
> **Estimated Complexity:** Medium (2-3 hours)

---

## Problem Statement

The library needs one-time initialization primitives for lazy static initialization and singleton patterns. These are commonly used for:

1. **Global Configuration**: Initialize once, read many times
2. **Singletons**: Ensure only one instance is created
3. **Lazy Statics**: Defer expensive initialization until first use
4. **Thread-Safe Initialization**: Guarantee exactly-once execution

---

## Proposed Solution

Implement two complementary types:

### 1. **Once** - Function Call Control
```rust
/// Ensures a function is called exactly once, even with concurrent access.
pub struct Once {
    // Internal state tracking
}
```

**Use Case:** Execute initialization code once
```rust
static INIT: Once = Once::new();
INIT.call_once(|| {
    // Expensive initialization here
});
```

### 2. **OnceCell<T>** - Lazy Value Initialization
```rust
/// A cell which can be written to only once.
pub struct OnceCell<T> {
    // Internal value storage
}
```

**Use Case:** Store a lazily-initialized value
```rust
static CONFIG: OnceCell<Config> = OnceCell::new();
let config = CONFIG.get_or_init(|| load_config());
```

---

## Public API Design

### Once API

```rust
impl Once {
    /// Creates a new `Once` value in the uninitialized state.
    pub const fn new() -> Self;
    
    /// Calls the given closure if this is the first call to `call_once`.
    ///
    /// If another thread is currently running initialization, this thread
    /// will block until initialization completes.
    pub fn call_once<F>(&self, f: F)
    where
        F: FnOnce();
    
    /// Returns true if initialization has completed.
    pub fn is_completed(&self) -> bool;
}
```

**Traits to implement:**
- `Default` - Returns `Once::new()`
- `Debug` - Shows completed state
- `Sync` - Can be shared across threads
- `Send` - Can be moved across threads

### OnceCell<T> API

```rust
impl<T> OnceCell<T> {
    /// Creates a new empty cell.
    pub const fn new() -> Self;
    
    /// Gets the contents of the cell, if initialized.
    pub fn get(&self) -> Option<&T>;
    
    /// Gets the contents, initializing if needed.
    pub fn get_or_init<F>(&self, f: F) -> &T
    where
        F: FnOnce() -> T;
    
    /// Gets the contents, initializing if needed (fallible version).
    pub fn get_or_try_init<F, E>(&self, f: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<T, E>;
    
    /// Sets the contents of the cell to `value`.
    ///
    /// Returns `Err(value)` if the cell was already initialized.
    pub fn set(&self, value: T) -> Result<(), T>;
    
    /// Takes the value out of the cell, leaving it empty.
    ///
    /// Returns `None` if the cell is empty, or `Some(value)` otherwise.
    pub fn take(&mut self) -> Option<T>;
    
    /// Consumes the cell, returning the wrapped value if initialized.
    pub fn into_inner(self) -> Option<T>;
}
```

**Traits to implement:**
- `Default` if `T: Default` - Returns empty cell
- `Debug` if `T: Debug` - Shows contents
- `Clone` if `T: Clone` - Clones contents
- `PartialEq` if `T: PartialEq`
- `From<T>` - Create initialized cell
- `Sync` if `T: Sync` - Can be shared
- `Send` if `T: Send` - Can be moved

---

## Implementation Strategy

### Approach 1: Re-export from parking_lot ✅ **RECOMMENDED**

**Reasoning:**
- `parking_lot` already has optimized Once and OnceCell
- Consistent with Mutex/RwLock implementation strategy
- No need to reimplement complex synchronization logic
- Battle-tested in production

**Code:**
```rust
// In src/once.rs
pub use parking_lot::Once;
pub use parking_lot::OnceCell;
```

### Approach 2: Implement from Scratch ❌ **NOT RECOMMENDED**

**Why not:**
- Complex state machine (uninitialized → initializing → initialized)
- Requires careful handling of concurrent access
- Need to handle panic during initialization
- More error-prone and time-consuming

---

## Test Cases

### Once Tests

1. **test_once_basic** - Single-threaded initialization
   ```rust
   let once = Once::new();
   let mut counter = 0;
   once.call_once(|| counter += 1);
   once.call_once(|| counter += 1); // Should not run
   assert_eq!(counter, 1);
   ```

2. **test_once_concurrent** - Multi-threaded initialization
   ```rust
   let once = Arc::new(Once::new());
   let counter = Arc::new(AtomicUsize::new(0));
   // Spawn multiple threads calling call_once
   // Assert counter == 1 after all threads complete
   ```

3. **test_once_is_completed** - State checking
   ```rust
   let once = Once::new();
   assert!(!once.is_completed());
   once.call_once(|| {});
   assert!(once.is_completed());
   ```

4. **test_once_panic_recovery** - Handle panics during init
   ```rust
   let once = Once::new();
   let _ = std::panic::catch_unwind(|| {
       once.call_once(|| panic!("init panic"));
   });
   // Subsequent call should work
   once.call_once(|| { /* successful init */ });
   ```

### OnceCell Tests

1. **test_oncecell_new** - Create empty cell
   ```rust
   let cell: OnceCell<i32> = OnceCell::new();
   assert_eq!(cell.get(), None);
   ```

2. **test_oncecell_get_or_init** - Lazy initialization
   ```rust
   let cell = OnceCell::new();
   let value = cell.get_or_init(|| 42);
   assert_eq!(*value, 42);
   assert_eq!(*cell.get_or_init(|| 99), 42); // Original value
   ```

3. **test_oncecell_set** - Direct initialization
   ```rust
   let cell = OnceCell::new();
   assert!(cell.set(42).is_ok());
   assert!(cell.set(99).is_err()); // Already initialized
   ```

4. **test_oncecell_concurrent** - Multi-threaded access
   ```rust
   let cell = Arc::new(OnceCell::new());
   // Spawn threads calling get_or_init
   // Assert all get same value
   ```

5. **test_oncecell_take** - Mutation
   ```rust
   let mut cell = OnceCell::new();
   cell.set(42).unwrap();
   assert_eq!(cell.take(), Some(42));
   assert_eq!(cell.get(), None);
   ```

6. **test_oncecell_into_inner** - Consumption
   ```rust
   let cell = OnceCell::new();
   cell.set(42).unwrap();
   assert_eq!(cell.into_inner(), Some(42));
   ```

---

## Acceptance Criteria

✅ **Functionality**
- [ ] Once ensures exactly-once execution
- [ ] OnceCell stores lazily-initialized value
- [ ] All public methods work as specified
- [ ] Concurrent access is thread-safe

✅ **Quality Gates**
- [ ] `cargo check` passes
- [ ] `cargo clippy -- -D warnings` passes (0 warnings)
- [ ] `cargo test` passes (all tests)
- [ ] `cargo doc --no-deps` passes (no doc warnings)
- [ ] `cargo fmt --check` passes

✅ **Documentation**
- [ ] All public items have doc comments
- [ ] Each doc comment includes usage example
- [ ] Module-level documentation explains purpose
- [ ] Examples compile and run correctly

✅ **Tests**
- [ ] At least 10 unit tests (5+ per type)
- [ ] Multi-threaded tests pass
- [ ] Edge cases covered (panic, concurrent init)
- [ ] Doc tests demonstrate usage

---

## Dependencies

```toml
[dependencies]
parking_lot = "0.12"  # Already in Cargo.toml
```

---

## Files to Create/Modify

### New Files
- `src/once.rs` - Once and OnceCell implementation and tests

### Modified Files
- `src/lib.rs` - Add `pub mod once;` and re-exports
- `docs/CHANGELOG.md` - Document new feature
- `docs/TODO.md` - Mark Once/OnceCell as complete

---

## Examples

### Example 1: Singleton Pattern
```rust
use prometheus_parking_lot::OnceCell;

static INSTANCE: OnceCell<Database> = OnceCell::new();

fn get_database() -> &'static Database {
    INSTANCE.get_or_init(|| {
        Database::connect("localhost:5432")
    })
}
```

### Example 2: Global Configuration
```rust
use prometheus_parking_lot::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static INIT: Once = Once::new();
static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn ensure_initialized() {
    INIT.call_once(|| {
        // Load config, set up logging, etc.
        INITIALIZED.store(true, Ordering::Release);
    });
}
```

---

## Success Metrics

| Metric | Target | Achieved |
|--------|--------|----------|
| Tests passing | 100% | ⏳ Pending |
| Clippy warnings | 0 | ⏳ Pending |
| Doc coverage | 100% | ⏳ Pending |
| Compilation time | < 5s | ⏳ Pending |
| Example compile | All pass | ⏳ Pending |

---

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| parking_lot API changes | High | Use specific version (0.12) |
| Panic during init | Medium | Test panic recovery scenarios |
| Performance regression | Low | Trust parking_lot's optimization |
| API incompatibility | Low | Follow parking_lot's API closely |

---

## Next Steps

1. ✅ Specification complete
2. ⏳ Create implementation plan (Phase 2: PLAN)
3. ⏳ Break into atomic tasks (Phase 3: TASKS)
4. ⏳ Execute implementation (Phase 4: EXECUTE)
5. ⏳ Verify completion (Phase 5: REFLECT)

---

*Specification complete. Ready for planning phase.*
