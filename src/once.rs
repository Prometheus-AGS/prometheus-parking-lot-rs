//! One-time initialization primitives.
//!
//! This module provides synchronization primitives for one-time initialization:
//!
//! - [`Once`] - Ensures a piece of code is executed exactly once (from `parking_lot`)
//! - [`OnceCell`] - A cell that can be written to only once (from `std::sync::OnceLock`)
//!
//! **Note:** `Once` is re-exported from the `parking_lot` crate for high performance.
//! `OnceCell` is re-exported from `std::sync::OnceLock` since `parking_lot` does not
//! provide a `OnceCell` type. Both provide thread-safe one-time initialization.
//!
//! These primitives are useful for:
//! - Global configuration initialization
//! - Singleton pattern implementation
//! - Lazy static values
//! - One-time setup operations
//!
//! # Examples
//!
//! ## Using `Once` for one-time initialization
//!
//! ```
//! use prometheus_parking_lot::Once;
//!
//! static INIT: Once = Once::new();
//!
//! INIT.call_once(|| {
//!     // This code runs only once, even if called from multiple threads
//!     println!("Initializing...");
//! });
//! ```
//!
//! ## Using `OnceCell` for lazy initialization
//!
//! ```
//! use prometheus_parking_lot::OnceCell;
//!
//! let cell = OnceCell::new();
//!
//! // First access initializes the value
//! let value = cell.get_or_init(|| {
//!     expensive_computation()
//! });
//!
//! // Subsequent accesses return the same value
//! let same_value = cell.get().unwrap();
//! # fn expensive_computation() -> i32 { 42 }
//! ```

// Re-export Once from parking_lot
pub use parking_lot::Once;

// Re-export std::sync::OnceLock as OnceCell
// Note: parking_lot does not provide OnceCell, so we use std::sync::OnceLock (Rust 1.70+)
// which provides equivalent thread-safe lazy initialization functionality.
pub use std::sync::OnceLock as OnceCell;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    /// Test basic `call_once` execution
    #[test]
    fn test_once_basic() {
        let once = Once::new();
        let mut value = 0;

        once.call_once(|| {
            value = 42;
        });

        assert_eq!(value, 42);
        assert_eq!(once.state(), parking_lot::OnceState::Done);
    }

    /// Test that `call_once` only executes once even with multiple calls
    #[test]
    fn test_once_idempotent() {
        let once = Once::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone1 = Arc::clone(&counter);
        once.call_once(move || {
            counter_clone1.fetch_add(1, Ordering::SeqCst);
        });

        let counter_clone2 = Arc::clone(&counter);
        once.call_once(move || {
            counter_clone2.fetch_add(1, Ordering::SeqCst);
        });

        let counter_clone3 = Arc::clone(&counter);
        once.call_once(move || {
            counter_clone3.fetch_add(1, Ordering::SeqCst);
        });

        // Should only have executed once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    /// Test `state()` tracking
    #[test]
    fn test_once_state() {
        let once = Once::new();

        // Before call_once
        assert_eq!(once.state(), parking_lot::OnceState::New);

        once.call_once(|| {
            // Do something
        });

        // After call_once
        assert_eq!(once.state(), parking_lot::OnceState::Done);
    }

    /// Test concurrent `call_once` from multiple threads
    #[test]
    fn test_once_concurrent() {
        let once = Arc::new(Once::new());
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        // Spawn 10 threads that all try to call_once
        for _ in 0..10 {
            let once_clone = Arc::clone(&once);
            let counter_clone = Arc::clone(&counter);

            let handle = thread::spawn(move || {
                once_clone.call_once(|| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                    // Simulate some work
                    thread::sleep(std::time::Duration::from_millis(1));
                });
            });

            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Should only have executed once despite 10 threads
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(once.state(), parking_lot::OnceState::Done);
    }

    // ========== OnceCell Tests ==========

    /// Test basic `get_or_init` lazy initialization
    #[test]
    fn test_oncecell_get_or_init() {
        let cell = OnceCell::new();

        // First access initializes
        let value = cell.get_or_init(|| 42);
        assert_eq!(*value, 42);

        // Subsequent accesses return the same value
        let same_value = cell.get_or_init(|| 100);
        assert_eq!(*same_value, 42); // Still 42, not 100!
    }

    /// Test `get` returns None before initialization, Some after
    #[test]
    fn test_oncecell_get_none() {
        let cell: OnceCell<i32> = OnceCell::new();

        // Before initialization
        assert!(cell.get().is_none());

        // Initialize
        cell.get_or_init(|| 42);

        // After initialization
        assert_eq!(cell.get(), Some(&42));
    }

    /// Test `set` on uninitialized cell succeeds
    #[test]
    fn test_oncecell_set_success() {
        let cell = OnceCell::new();

        // Set on empty cell should succeed
        let result = cell.set(42);
        assert!(result.is_ok());
        assert_eq!(cell.get(), Some(&42));
    }

    /// Test `set` on already initialized cell fails
    #[test]
    fn test_oncecell_set_fail() {
        let cell = OnceCell::new();

        // First set succeeds
        assert!(cell.set(42).is_ok());

        // Second set fails and returns the value back
        let result = cell.set(100);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), 100);

        // Original value unchanged
        assert_eq!(cell.get(), Some(&42));
    }

    /// Test `take` and `into_inner` ownership operations
    #[test]
    fn test_oncecell_take_into_inner() {
        // Test take()
        let mut cell = OnceCell::new();
        cell.set(42).unwrap();

        let value = cell.take();
        assert_eq!(value, Some(42));
        assert!(cell.get().is_none()); // Cell is now empty

        // Test into_inner()
        let cell2 = OnceCell::new();
        cell2.set(100).unwrap();

        let value2 = cell2.into_inner();
        assert_eq!(value2, Some(100));
    }

    /// Test concurrent `get_or_init` from multiple threads
    #[test]
    fn test_oncecell_concurrent() {
        let cell = Arc::new(OnceCell::new());
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        // Spawn 10 threads that all try to get_or_init
        for _ in 0..10 {
            let cell_clone = Arc::clone(&cell);
            let counter_clone = Arc::clone(&counter);

            let handle = thread::spawn(move || {
                let value = cell_clone.get_or_init(|| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                    // Simulate expensive computation
                    thread::sleep(std::time::Duration::from_millis(1));
                    42
                });

                assert_eq!(*value, 42);
            });

            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Initialization should only happen once despite 10 threads
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(cell.get(), Some(&42));
    }
}
