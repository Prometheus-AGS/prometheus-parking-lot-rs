//! Reader-writer lock implementation
//!
//! This module provides a high-performance reader-writer lock (`RwLock`) that allows
//! multiple concurrent readers or a single writer. It's built on top of the battle-tested
//! `parking_lot` crate.
//!
//! # Features
//!
//! - Multiple concurrent readers
//! - Exclusive writer access
//! - Fair lock acquisition
//! - No poisoning on panic
//! - Supports both read and write guards
//!
//! # Examples
//!
//! Basic usage:
//!
//! ```
//! use prometheus_parking_lot::RwLock;
//!
//! let lock = RwLock::new(5);
//!
//! // Many reader locks can be held at once
//! {
//!     let r1 = lock.read();
//!     let r2 = lock.read();
//!     assert_eq!(*r1, 5);
//!     assert_eq!(*r2, 5);
//! } // read locks are dropped
//!
//! // Only one write lock can be held
//! {
//!     let mut w = lock.write();
//!     *w += 1;
//!     assert_eq!(*w, 6);
//! } // write lock is dropped
//! ```
//!
//! Concurrent access pattern:
//!
//! ```
//! use prometheus_parking_lot::RwLock;
//! use std::sync::Arc;
//! use std::thread;
//!
//! let lock = Arc::new(RwLock::new(0));
//! let mut handles = vec![];
//!
//! // Spawn 10 reader threads
//! for _ in 0..10 {
//!     let lock = Arc::clone(&lock);
//!     handles.push(thread::spawn(move || {
//!         let value = lock.read();
//!         println!("Read: {}", *value);
//!     }));
//! }
//!
//! // Spawn 1 writer thread
//! let lock_clone = Arc::clone(&lock);
//! handles.push(thread::spawn(move || {
//!     let mut value = lock_clone.write();
//!     *value += 1;
//! }));
//!
//! for handle in handles {
//!     handle.join().unwrap();
//! }
//! ```

// Re-export parking_lot's RwLock types
pub use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard,
    RwLockUpgradableReadGuard, RwLockWriteGuard,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_rwlock_new() {
        let lock = RwLock::new(42);
        assert_eq!(*lock.read(), 42);
    }

    #[test]
    fn test_read_write() {
        let lock = RwLock::new(0);

        // Read
        {
            let r = lock.read();
            assert_eq!(*r, 0);
        }

        // Write
        {
            let mut w = lock.write();
            *w = 42;
        }

        // Read again
        {
            let r = lock.read();
            assert_eq!(*r, 42);
        }
    }

    #[test]
    fn test_multiple_readers() {
        let lock = RwLock::new(5);

        let r1 = lock.read();
        let r2 = lock.read();
        let r3 = lock.read();

        assert_eq!(*r1, 5);
        assert_eq!(*r2, 5);
        assert_eq!(*r3, 5);
    }

    #[test]
    fn test_concurrent_reads() {
        let lock = Arc::new(RwLock::new(100));
        let mut handles = vec![];

        for _ in 0..10 {
            let lock = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                let value = lock.read();
                assert_eq!(*value, 100);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_write_exclusion() {
        let lock = Arc::new(RwLock::new(0));
        let lock_clone = Arc::clone(&lock);

        let writer = thread::spawn(move || {
            let mut w = lock_clone.write();
            *w += 1;
            thread::sleep(std::time::Duration::from_millis(50));
            *w += 1;
        });

        thread::sleep(std::time::Duration::from_millis(10));

        let _final_value = {
            let r = lock.read();
            *r
        };

        writer.join().unwrap();

        let after_write = *lock.read();
        assert_eq!(after_write, 2);
    }

    #[test]
    fn test_write_then_read() {
        let lock = RwLock::new(0);

        {
            let mut w = lock.write();
            *w = 10;
        }

        {
            let r = lock.read();
            assert_eq!(*r, 10);
        }
    }

    #[test]
    fn test_try_read() {
        let lock = RwLock::new(5);

        if let Some(r) = lock.try_read() {
            assert_eq!(*r, 5);
            drop(r);
        };
    }

    #[test]
    fn test_try_write() {
        let lock = RwLock::new(5);

        if let Some(mut w) = lock.try_write() {
            *w = 10;
        } else {
            panic!("Should be able to acquire write lock");
        }

        assert_eq!(*lock.read(), 10);
    }

    #[test]
    fn test_into_inner() {
        let lock = RwLock::new(42);
        let value = lock.into_inner();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_get_mut() {
        let mut lock = RwLock::new(0);
        *lock.get_mut() = 42;
        assert_eq!(*lock.read(), 42);
    }
}
