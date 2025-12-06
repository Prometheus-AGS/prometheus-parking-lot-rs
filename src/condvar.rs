//! Condition variable implementation.
//!
//! This module provides a condition variable type that can be used for thread coordination.

use crate::MutexGuard;
use parking_lot;

/// A condition variable.
///
/// Condition variables represent the ability to block a thread such that
/// it consumes no CPU time while waiting for an event to occur. Unlike
/// `std::sync::Condvar`, this type does not implement poisoning.
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
/// // Spawn a thread that will signal the condition variable
/// thread::spawn(move || {
///     let (lock, cvar) = &*pair2;
///     let mut started = lock.lock();
///     *started = true;
///     // Signal that we're ready
///     cvar.notify_one();
/// });
///
/// // Wait for the thread to start up
/// let (lock, cvar) = &*pair;
/// let mut started = lock.lock();
/// while !*started {
///     cvar.wait(&mut started);
/// }
/// ```
#[derive(Debug, Default)]
pub struct Condvar {
    inner: parking_lot::Condvar,
}

impl Condvar {
    /// Creates a new condition variable.
    ///
    /// # Examples
    ///
    /// ```
    /// use prometheus_parking_lot::Condvar;
    ///
    /// let cvar = Condvar::new();
    /// ```
    #[inline]
    pub const fn new() -> Self {
        Self {
            inner: parking_lot::Condvar::new(),
        }
    }

    /// Blocks the current thread until this condition variable receives a notification.
    ///
    /// This function will atomically unlock the mutex specified by `guard` and block the
    /// current thread. This means that any calls to `notify_one` or `notify_all` which
    /// happen logically after the mutex is unlocked are candidates to wake this thread up.
    /// When this function call returns, the lock specified will have been re-acquired.
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
    ///     let mut ready = lock.lock();
    ///     *ready = true;
    ///     cvar.notify_one();
    /// });
    ///
    /// let (lock, cvar) = &*pair;
    /// let mut ready = lock.lock();
    /// while !*ready {
    ///     cvar.wait(&mut ready);
    /// }
    /// ```
    #[inline]
    pub fn wait<T>(&self, guard: &mut MutexGuard<'_, T>) {
        self.inner.wait(guard);
    }

    /// Blocks the current thread until this condition variable receives a notification
    /// and the provided condition is met.
    ///
    /// This function will atomically unlock the mutex and block the current thread until
    /// `condition` returns `false` after being called with the mutex guard.
    ///
    /// # Examples
    ///
    /// ```
    /// use prometheus_parking_lot::{Mutex, Condvar};
    /// use std::sync::Arc;
    /// use std::thread;
    ///
    /// let pair = Arc::new((Mutex::new(0), Condvar::new()));
    /// let pair2 = Arc::clone(&pair);
    ///
    /// thread::spawn(move || {
    ///     let (lock, cvar) = &*pair2;
    ///     let mut count = lock.lock();
    ///     *count = 10;
    ///     cvar.notify_one();
    /// });
    ///
    /// let (lock, cvar) = &*pair;
    /// let mut count = lock.lock();
    /// cvar.wait_while(&mut count, |c| *c < 10);
    /// assert_eq!(*count, 10);
    /// ```
    #[inline]
    pub fn wait_while<T, F>(&self, guard: &mut MutexGuard<'_, T>, mut condition: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        self.inner.wait_while(guard, condition);
    }

    /// Wakes up one blocked thread on this condvar.
    ///
    /// If there is a blocked thread on this condition variable, then it will be woken up.
    /// Calls to `notify_one` are not buffered in any way.
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
    #[inline]
    pub fn notify_one(&self) {
        self.inner.notify_one();
    }

    /// Wakes up all blocked threads on this condvar.
    ///
    /// This method will ensure that any current waiters on the condition variable are awoken.
    /// Calls to `notify_all` are not buffered in any way.
    ///
    /// # Examples
    ///
    /// ```
    /// use prometheus_parking_lot::{Mutex, Condvar};
    /// use std::sync::Arc;
    /// use std::thread;
    ///
    /// let pair = Arc::new((Mutex::new(false), Condvar::new()));
    /// let mut handles = vec![];
    ///
    /// for _ in 0..3 {
    ///     let pair = Arc::clone(&pair);
    ///     let handle = thread::spawn(move || {
    ///         let (lock, cvar) = &*pair;
    ///         let mut started = lock.lock();
    ///         while !*started {
    ///             cvar.wait(&mut started);
    ///         }
    ///     });
    ///     handles.push(handle);
    /// }
    ///
    /// // Let threads start
    /// std::thread::sleep(std::time::Duration::from_millis(10));
    ///
    /// // Wake them all up
    /// {
    ///     let (lock, cvar) = &*pair;
    ///     let mut started = lock.lock();
    ///     *started = true;
    ///     cvar.notify_all();
    /// }
    ///
    /// // Wait for all threads
    /// for handle in handles {
    ///     handle.join().unwrap();
    /// }
    /// ```
    #[inline]
    pub fn notify_all(&self) {
        self.inner.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mutex;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_basic_wait_notify() {
        let pair = Arc::new((Mutex::new(false), Condvar::new()));
        let pair2 = Arc::clone(&pair);

        thread::spawn(move || {
            let (lock, cvar) = &*pair2;
            thread::sleep(Duration::from_millis(10));
            let mut started = lock.lock();
            *started = true;
            cvar.notify_one();
        });

        let (lock, cvar) = &*pair;
        let mut started = lock.lock();
        while !*started {
            cvar.wait(&mut started);
        }
        assert!(*started);
    }

    #[test]
    fn test_notify_one() {
        let pair = Arc::new((Mutex::new(0), Condvar::new()));
        let pair2 = Arc::clone(&pair);
        let pair3 = Arc::clone(&pair);

        // Start two waiting threads
        let handle1 = thread::spawn(move || {
            let (lock, cvar) = &*pair2;
            let mut count = lock.lock();
            while *count == 0 {
                cvar.wait(&mut count);
            }
        });

        let handle2 = thread::spawn(move || {
            let (lock, cvar) = &*pair3;
            let mut count = lock.lock();
            while *count == 0 {
                cvar.wait(&mut count);
            }
        });

        thread::sleep(Duration::from_millis(10));

        // Notify one thread
        {
            let (lock, cvar) = &*pair;
            let mut count = lock.lock();
            *count = 1;
            cvar.notify_one();
        }

        thread::sleep(Duration::from_millis(10));

        // Notify the other thread
        {
            let (lock, cvar) = &*pair;
            let mut count = lock.lock();
            *count = 2;
            cvar.notify_one();
        }

        handle1.join().unwrap();
        handle2.join().unwrap();
    }

    #[test]
    fn test_notify_all() {
        let pair = Arc::new((Mutex::new(false), Condvar::new()));
        let mut handles = vec![];

        for _ in 0..5 {
            let pair = Arc::clone(&pair);
            let handle = thread::spawn(move || {
                let (lock, cvar) = &*pair;
                let mut started = lock.lock();
                while !*started {
                    cvar.wait(&mut started);
                }
            });
            handles.push(handle);
        }

        thread::sleep(Duration::from_millis(10));

        // Wake all threads at once
        {
            let (lock, cvar) = &*pair;
            let mut started = lock.lock();
            *started = true;
            cvar.notify_all();
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_wait_while() {
        let pair = Arc::new((Mutex::new(0), Condvar::new()));
        let pair2 = Arc::clone(&pair);

        thread::spawn(move || {
            let (lock, cvar) = &*pair2;
            for i in 1..=10 {
                thread::sleep(Duration::from_millis(5));
                let mut count = lock.lock();
                *count = i;
                cvar.notify_one();
            }
        });

        let (lock, cvar) = &*pair;
        let mut count = lock.lock();
        cvar.wait_while(&mut count, |c| *c < 10);
        assert_eq!(*count, 10);
    }

    #[test]
    fn test_spurious_wakeup_handling() {
        // This test verifies that wait is used in a loop to handle spurious wakeups
        let pair = Arc::new((Mutex::new(false), Condvar::new()));
        let pair2 = Arc::clone(&pair);

        thread::spawn(move || {
            let (lock, cvar) = &*pair2;
            thread::sleep(Duration::from_millis(20));
            let mut ready = lock.lock();
            *ready = true;
            cvar.notify_one();
        });

        let (lock, cvar) = &*pair;
        let mut ready = lock.lock();
        // Use a while loop to handle potential spurious wakeups
        while !*ready {
            cvar.wait(&mut ready);
        }
        assert!(*ready);
    }

    #[test]
    fn test_producer_consumer() {
        let pair = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let pair2 = Arc::clone(&pair);

        // Producer thread
        let producer = thread::spawn(move || {
            let (lock, cvar) = &*pair2;
            for i in 0..5 {
                thread::sleep(Duration::from_millis(5));
                let mut queue = lock.lock();
                queue.push(i);
                cvar.notify_one();
            }
        });

        // Consumer thread
        let (lock, cvar) = &*pair;
        let mut consumed = Vec::new();
        while consumed.len() < 5 {
            let mut queue = lock.lock();
            while queue.is_empty() {
                cvar.wait(&mut queue);
            }
            consumed.push(queue.remove(0));
        }

        producer.join().unwrap();
        assert_eq!(consumed, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_multiple_waiters() {
        let pair = Arc::new((Mutex::new(0), Condvar::new()));
        let mut handles = vec![];

        // Spawn 10 waiting threads
        for _ in 0..10 {
            let pair = Arc::clone(&pair);
            let handle = thread::spawn(move || {
                let (lock, cvar) = &*pair;
                let mut count = lock.lock();
                while *count < 10 {
                    cvar.wait(&mut count);
                }
                *count
            });
            handles.push(handle);
        }

        thread::sleep(Duration::from_millis(10));

        // Increment and notify
        {
            let (lock, cvar) = &*pair;
            let mut count = lock.lock();
            *count = 10;
            cvar.notify_all();
        }

        // All threads should wake up and see count = 10
        for handle in handles {
            let result = handle.join().unwrap();
            assert_eq!(result, 10);
        }
    }

    #[test]
    fn test_notify_before_wait() {
        // Edge case: notify is called before wait
        let pair = Arc::new((Mutex::new(true), Condvar::new()));
        let (lock, cvar) = &*pair;

        // Notify before anyone is waiting
        cvar.notify_one();

        // This should not block since the condition is already true
        let mut ready = lock.lock();
        if !*ready {
            cvar.wait(&mut ready);
        }
        assert!(*ready);
    }

    #[test]
    fn test_condvar_with_mutex() {
        // Integration test: Condvar works correctly with our Mutex
        let mutex = Arc::new(Mutex::new(0));
        let cvar = Arc::new(Condvar::new());

        let mutex2 = Arc::clone(&mutex);
        let cvar2 = Arc::clone(&cvar);

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            let mut data = mutex2.lock();
            *data = 42;
            cvar2.notify_one();
        });

        let mut data = mutex.lock();
        while *data != 42 {
            cvar.wait(&mut data);
        }
        assert_eq!(*data, 42);
    }
}
