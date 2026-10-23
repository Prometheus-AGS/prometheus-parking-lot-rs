# Prometheus Parking Lot - API Specification

> **Status:** Living Document  
> **Last Updated:** 2024-12-06  
> **Version:** 0.1.0-draft

---

## Module Structure

```rust
pub mod prometheus_parking_lot {
    // Core synchronization primitives
    pub struct Mutex<T>;
    pub struct MutexGuard<'a, T>;
    
    pub struct RwLock<T>;
    pub struct RwLockReadGuard<'a, T>;
    pub struct RwLockWriteGuard<'a, T>;
    
    pub struct Condvar;
    
    // Advanced types (Phase 2)
    pub struct Once;
    pub struct OnceCell<T>;
    pub struct ReentrantMutex<T>;
    pub struct FairMutex<T>;
    
    // Re-exports from lock_api
    pub use lock_api::{MappedMutexGuard, MappedRwLockReadGuard, MappedRwLockWriteGuard};
}
```

---

## Mutex<T>

### Type Definition

```rust
/// A mutual exclusion primitive useful for protecting shared data.
///
/// This mutex will block threads waiting for the lock to become available.
/// Unlike `std::sync::Mutex`, this type does not implement poisoning.
///
/// # Examples
///
/// ```
/// use prometheus_parking_lot::Mutex;
/// use std::sync::Arc;
/// use std::thread;
///
/// let mutex = Arc::new(Mutex::new(0));
/// let mut handles = vec![];
///
/// for _ in 0..10 {
///     let mutex = Arc::clone(&mutex);
///     let handle = thread::spawn(move || {
///         let mut num = mutex.lock();
///         *num += 1;
///     });
///     handles.push(handle);
/// }
///
/// for handle in handles {
///     handle.join().unwrap();
/// }
///
/// assert_eq!(*mutex.lock(), 10);
/// ```
pub struct Mutex<T: ?Sized> {
    // Internal implementation
}
```

### Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `const fn new(value: T) -> Mutex<T>` | Creates a new unlocked mutex |
| `lock` | `fn lock(&self) -> MutexGuard<'_, T>` | Acquires the lock, blocking if necessary |
| `try_lock` | `fn try_lock(&self) -> Option<MutexGuard<'_, T>>` | Attempts to acquire without blocking |
| `get_mut` | `fn get_mut(&mut self) -> &mut T` | Returns mutable reference (requires &mut self) |
| `into_inner` | `fn into_inner(self) -> T` | Consumes mutex, returns data |
| `is_locked` | `fn is_locked(&self) -> bool` | Checks if currently locked |

### Trait Implementations

- `Send` if `T: Send`
- `Sync` if `T: Send`
- `Default` if `T: Default`
- `Debug`
- `From<T>`

---

## RwLock<T>

### Type Definition

```rust
/// A reader-writer lock.
///
/// This type of lock allows a number of readers or at most one writer
/// at any point in time.
///
/// # Examples
///
/// ```
/// use prometheus_parking_lot::RwLock;
///
/// let lock = RwLock::new(5);
///
/// // Multiple readers can hold the lock simultaneously
/// {
///     let r1 = lock.read();
///     let r2 = lock.read();
///     assert_eq!(*r1, 5);
///     assert_eq!(*r2, 5);
/// }
///
/// // Only one writer can hold the lock
/// {
///     let mut w = lock.write();
///     *w += 1;
///     assert_eq!(*w, 6);
/// }
/// ```
pub struct RwLock<T: ?Sized> {
    // Internal implementation
}
```

### Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `const fn new(value: T) -> RwLock<T>` | Creates a new unlocked RwLock |
| `read` | `fn read(&self) -> RwLockReadGuard<'_, T>` | Acquires a read lock |
| `write` | `fn write(&self) -> RwLockWriteGuard<'_, T>` | Acquires a write lock |
| `try_read` | `fn try_read(&self) -> Option<RwLockReadGuard<'_, T>>` | Attempts to acquire read lock |
| `try_write` | `fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>>` | Attempts to acquire write lock |
| `get_mut` | `fn get_mut(&mut self) -> &mut T` | Returns mutable reference |
| `into_inner` | `fn into_inner(self) -> T` | Consumes lock, returns data |

---

## Condvar

### Type Definition

```rust
/// A condition variable.
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
    // Internal implementation
}
```

### Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `const fn new() -> Condvar` | Creates a new condition variable |
| `wait` | `fn wait<T>(&self, guard: &mut MutexGuard<'_, T>)` | Blocks until notified |
| `wait_while` | `fn wait_while<T, F>(&self, guard: &mut MutexGuard<'_, T>, condition: F)` | Blocks while condition is true |
| `notify_one` | `fn notify_one(&self)` | Wakes up one waiting thread |
| `notify_all` | `fn notify_all(&self)` | Wakes up all waiting threads |

---

## Guard Types

### MutexGuard<'a, T>

```rust
/// An RAII guard returned by Mutex::lock.
///
/// When this structure is dropped, the lock will be released.
pub struct MutexGuard<'a, T: ?Sized> {
    // Internal implementation
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {}
```

### RwLockReadGuard<'a, T>

```rust
/// An RAII guard for read access to an RwLock.
pub struct RwLockReadGuard<'a, T: ?Sized> {
    // Internal implementation
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;
}
```

### RwLockWriteGuard<'a, T>

```rust
/// An RAII guard for write access to an RwLock.
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    // Internal implementation  
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {}
```

---

## Error Handling

This crate **does not use poisoning**. Methods that can fail return `Option`:

| Method | Return Type | Failure Case |
|--------|-------------|--------------|
| `try_lock` | `Option<MutexGuard>` | Lock already held |
| `try_read` | `Option<RwLockReadGuard>` | Write lock held |
| `try_write` | `Option<RwLockWriteGuard>` | Any lock held |

---

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Enable standard library support | ✅ Yes |
| `nightly` | Enable nightly-only optimizations | ❌ No |

---

## Safety Guarantees

1. **No Data Races**: All types are `Send + Sync` only when safe
2. **No Deadlocks**: Single-lock operations cannot deadlock
3. **No Undefined Behavior**: All code is safe Rust (no `unsafe` unless documented)
4. **Memory Safety**: Guards ensure locks are released on drop

---

## Comparison with std::sync

| Feature | prometheus_parking_lot | std::sync |
|---------|----------------------|-----------|
| Poisoning | ❌ No | ✅ Yes |
| Lock size | 1 byte | 40+ bytes |
| `const fn new` | ✅ Yes | ✅ Yes (Rust 1.63+) |
| Performance | Faster (uncontended) | Baseline |

---

*This is a living document. Update as API evolves.*
