//! Mutex implementation
//!
//! This module provides a high-performance mutual exclusion primitive built on top of
//! the battle-tested `parking_lot` crate.
//!
//! # Features
//!
//! - Fair lock acquisition
//! - No poisoning on panic
//! - Compact memory footprint
//! - Fast lock/unlock operations
//!
//! # Examples
//!
//! Basic usage:
//!
//! ```
//! use prometheus_parking_lot::Mutex;
//!
//! let mutex = Mutex::new(0);
//! *mutex.lock() = 10;
//! assert_eq!(*mutex.lock(), 10);
//! ```
//!
//! Concurrent access:
//!
//! ```
//! use prometheus_parking_lot::Mutex;
//! use std::sync::Arc;
//! use std::thread;
//!
//! let mutex = Arc::new(Mutex::new(0));
//! let mut handles = vec![];
//!
//! for _ in 0..10 {
//!     let mutex = Arc::clone(&mutex);
//!     handles.push(thread::spawn(move || {
//!         let mut num = mutex.lock();
//!         *num += 1;
//!     }));
//! }
//!
//! for handle in handles {
//!     handle.join().unwrap();
//! }
//!
//! assert_eq!(*mutex.lock(), 10);
//! ```

// Re-export parking_lot's Mutex types
pub use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_mutex_new() {
        let mutex = Mutex::new(42);
        assert_eq!(*mutex.lock(), 42);
    }

    #[test]
    fn test_mutex_lock_unlock() {
        let mutex = Mutex::new(0);

        {
            let mut guard = mutex.lock();
            *guard = 10;
        }

        assert_eq!(*mutex.lock(), 10);
    }

    #[test]
    fn test_mutex_concurrent_access() {
        let mutex = Arc::new(Mutex::new(0));
        let mut handles = vec![];

        for _ in 0..10 {
            let mutex = Arc::clone(&mutex);
            handles.push(thread::spawn(move || {
                let mut num = mutex.lock();
                *num += 1;
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(*mutex.lock(), 10);
    }

    #[test]
    fn test_mutex_try_lock() {
        let mutex = Mutex::new(5);

        if let Some(mut guard) = mutex.try_lock() {
            *guard = 10;
        } else {
            panic!("Should be able to acquire lock");
        }

        assert_eq!(*mutex.lock(), 10);
    }

    #[test]
    fn test_mutex_into_inner() {
        let mutex = Mutex::new(42);
        let value = mutex.into_inner();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_mutex_get_mut() {
        let mut mutex = Mutex::new(0);
        *mutex.get_mut() = 42;
        assert_eq!(*mutex.lock(), 42);
    }

    #[test]
    fn test_mutex_is_locked() {
        let mutex = Mutex::new(0);
        assert!(!mutex.is_locked());

        let _guard = mutex.lock();
        assert!(mutex.is_locked());
    }
}
