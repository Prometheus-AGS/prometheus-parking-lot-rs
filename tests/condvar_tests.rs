//! Integration tests for Condvar
//!
//! These tests verify that Condvar works correctly in realistic scenarios.

use prometheus_parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Test a simple producer-consumer pattern
#[test]
fn test_producer_consumer_queue() {
    const NUM_ITEMS: usize = 100;

    let queue = Arc::new((Mutex::new(Vec::<usize>::new()), Condvar::new()));
    let queue_producer = Arc::clone(&queue);
    let queue_consumer = Arc::clone(&queue);

    // Producer thread
    let producer = thread::spawn(move || {
        for i in 0..NUM_ITEMS {
            let (lock, cvar) = &*queue_producer;
            lock.lock().push(i);
            cvar.notify_one();
        }
    });

    // Consumer thread
    let consumer = thread::spawn(move || {
        let mut received = Vec::new();
        let (lock, cvar) = &*queue_consumer;

        while received.len() < NUM_ITEMS {
            let mut queue = lock.lock();
            while queue.is_empty() {
                cvar.wait(&mut queue);
            }
            while let Some(item) = queue.pop() {
                received.push(item);
            }
        }

        received.sort();
        received
    });

    producer.join().unwrap();
    let result = consumer.join().unwrap();

    assert_eq!(result.len(), NUM_ITEMS);
    for (i, &val) in result.iter().enumerate() {
        assert_eq!(val, i);
    }
}

/// Test barrier-like synchronization with notify_all
#[test]
fn test_barrier_pattern() {
    const NUM_THREADS: usize = 5;

    let state = Arc::new((Mutex::new(false), Condvar::new()));
    let mut handles = vec![];

    for _ in 0..NUM_THREADS {
        let state = Arc::clone(&state);
        handles.push(thread::spawn(move || {
            let (lock, cvar) = &*state;
            let mut ready = lock.lock();
            while !*ready {
                cvar.wait(&mut ready);
            }
            // All threads should see ready = true
            assert!(*ready);
        }));
    }

    // Give threads time to start waiting
    thread::sleep(Duration::from_millis(50));

    // Release all threads at once
    {
        let (lock, cvar) = &*state;
        *lock.lock() = true;
        cvar.notify_all();
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test timeout behavior
#[test]
fn test_wait_timeout() {
    let mutex = Mutex::new(());
    let condvar = Condvar::new();

    let mut guard = mutex.lock();
    let result = condvar.wait_for(&mut guard, Duration::from_millis(10));

    assert!(result.timed_out());
}

/// Test that wait releases the lock while waiting
#[test]
fn test_wait_releases_lock() {
    let pair = Arc::new((Mutex::new(0), Condvar::new()));
    let pair2 = Arc::clone(&pair);

    let handle = thread::spawn(move || {
        let (lock, cvar) = &*pair2;
        let mut guard = lock.lock();
        // This thread will wait, releasing the lock
        cvar.wait(&mut guard);
        *guard
    });

    // Give the thread time to start waiting
    thread::sleep(Duration::from_millis(50));

    // We should be able to acquire the lock while the other thread is waiting
    {
        let (lock, cvar) = &*pair;
        let mut guard = lock.lock();
        *guard = 42;
        cvar.notify_one();
    }

    let result = handle.join().unwrap();
    assert_eq!(result, 42);
}

/// Test multiple producer, single consumer
#[test]
fn test_mpsc_pattern() {
    const NUM_PRODUCERS: usize = 4;
    const ITEMS_PER_PRODUCER: usize = 25;

    let queue = Arc::new((Mutex::new(Vec::<(usize, usize)>::new()), Condvar::new()));
    let mut handles = vec![];

    // Spawn producer threads
    for producer_id in 0..NUM_PRODUCERS {
        let queue = Arc::clone(&queue);
        handles.push(thread::spawn(move || {
            for item in 0..ITEMS_PER_PRODUCER {
                let (lock, cvar) = &*queue;
                lock.lock().push((producer_id, item));
                cvar.notify_one();
            }
        }));
    }

    // Consumer collects all items
    let expected_total = NUM_PRODUCERS * ITEMS_PER_PRODUCER;
    let mut received = Vec::new();
    let (lock, cvar) = &*queue;

    while received.len() < expected_total {
        let mut queue = lock.lock();
        while queue.is_empty() && received.len() < expected_total {
            cvar.wait(&mut queue);
        }
        while let Some(item) = queue.pop() {
            received.push(item);
        }
    }

    // Wait for all producers to finish
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(received.len(), expected_total);
}
