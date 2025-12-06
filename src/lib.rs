//! # Prometheus Parking Lot
//!
//! High-performance synchronization primitives for the Prometheus AI Platform.
//!
//! This library provides efficient, lightweight synchronization primitives built on top of
//! the battle-tested `parking_lot` crate.
//!
//! ## Modules
//!
//! - [`mutex`] - Mutual exclusion primitive
//! - [`rwlock`] - Reader-writer lock
//! - [`condvar`] - Condition variable for thread coordination
//! - [`once`] - One-time initialization primitives (`Once`, `OnceCell`)
//!
//! ## Examples
//!
//! ### Using Mutex
//!
//! ```
//! use prometheus_parking_lot::Mutex;
//!
//! let mutex = Mutex::new(0);
//! *mutex.lock() = 10;
//! assert_eq!(*mutex.lock(), 10);
//! ```
//!
//! ### Using `RwLock`
//!
//! ```
//! use prometheus_parking_lot::RwLock;
//!
//! let lock = RwLock::new(5);
//!
//! // Multiple readers
//! let r1 = lock.read();
//! let r2 = lock.read();
//! assert_eq!(*r1, 5);
//! assert_eq!(*r2, 5);
//! drop(r1);
//! drop(r2);
//!
//! // One writer
//! let mut w = lock.write();
//! *w = 10;
//! ```
//!
//! ### Using Condvar for Thread Coordination
//!
//! ```
//! use prometheus_parking_lot::{Mutex, Condvar};
//! use std::sync::Arc;
//! use std::thread;
//!
//! let pair = Arc::new((Mutex::new(false), Condvar::new()));
//! let pair2 = Arc::clone(&pair);
//!
//! // Spawn a thread that will signal when ready
//! thread::spawn(move || {
//!     let (lock, cvar) = &*pair2;
//!     let mut ready = lock.lock();
//!     *ready = true;
//!     cvar.notify_one();
//! });
//!
//! // Wait for the signal
//! let (lock, cvar) = &*pair;
//! let mut ready = lock.lock();
//! while !*ready {
//!     cvar.wait(&mut ready);
//! }
//! assert!(*ready);
//! ```

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]

pub mod condvar;
pub mod mutex;
pub mod once;
pub mod rwlock;

// Re-export main types for convenience
pub use condvar::Condvar;
pub use mutex::{Mutex, MutexGuard};
pub use once::{Once, OnceCell};
pub use rwlock::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};
