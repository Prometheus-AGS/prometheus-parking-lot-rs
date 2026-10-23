//! Result slots of the native `WorkerPool` (cr-22).
//!
//! One slot per submitted task, keyed by the pool's `u64` task id. A slot is
//! `Pending` until its worker stores the task's outcome, then `Ready` until it
//! is taken or its TTL runs out, then an `Expired` tombstone for one more TTL
//! so a late caller learns the result expired instead of "never existed".
//!
//! # Waiting without blocking threads
//!
//! A waiter registers a [`oneshot::Sender`] in the pending slot and waits on
//! the receiver. An async waiter awaits it under a tokio timer, so it is a
//! suspended future, not a parked thread: any number of `retrieve_async`
//! callers occupy no blocking-pool thread (scenario S-17). The blocking
//! `retrieve` parks its own calling thread on the same receiver with a
//! deadline, and refuses (through tokio's own check) to do so on a thread
//! that drives async tasks.
//!
//! # No lost wakeup
//!
//! Every state change and every waiter registration happens under the one
//! table [`Mutex`]. `fill` moves `Pending -> Ready` and takes the waiter list
//! in one critical section; a waiter checks the state and registers in one
//! critical section. So a waiter either sees `Ready`, or is in the list `fill`
//! takes and is sent to. Happens-before: the outcome written by `fill` before
//! its unlock is visible to the waiter that locks the table after being woken
//! (mutex unlock synchronizes-with the next lock). No atomics are used here.
//!
//! # TTL sweeping
//!
//! Expiry is measured on the pool's [`Clock::monotonic`] reading, so tests
//! drive it with a `TestClock`. A min-heap of `(when, id)` holds each ready
//! slot's expiry and each tombstone's removal time. Every table operation
//! first pops the entries that are due, so the cost is proportional to what
//! expires. An idle pool sweeps from its worker threads, which wake from
//! their channel wait every sweep interval (no extra thread).
//!
//! [`Clock::monotonic`]: prometheus_parking_lot_core::Clock::monotonic

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::oneshot;

/// A slot's key: the pool's task id.
pub(super) type SlotId = u64;

/// Why a wait produced no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Missing {
    /// No slot: never created, already taken, or its tombstone is gone.
    NotFound,
    /// The result was stored but not taken within its TTL and was evicted.
    Expired,
    /// The caller's wait ended before the result was stored. The slot stays,
    /// so a later call can still take the result (until its TTL runs out).
    TimedOut,
}

/// One task's result slot.
enum Slot<V> {
    /// The task has not finished; these waiters are woken when it does.
    Pending { waiters: Vec<oneshot::Sender<()>> },
    /// The outcome, kept until taken or until `expires_at`.
    Ready { value: V, expires_at: Duration },
    /// The outcome was evicted unread; reported once as [`Missing::Expired`].
    Expired,
}

/// What a lookup found.
enum Lookup<V> {
    /// The value; the slot is now gone.
    Taken(V),
    /// Nothing to wait for.
    Missing(Missing),
    /// Still pending; the receiver fires (or closes) when it is filled.
    Wait(oneshot::Receiver<()>),
}

/// The slots and their expiry schedule, guarded together.
struct Table<V> {
    slots: HashMap<SlotId, Slot<V>>,
    /// `(when, id)`: a ready slot's expiry, or a tombstone's removal time.
    due: BinaryHeap<Reverse<(Duration, SlotId)>>,
    /// How long a ready result (and then its tombstone) is kept.
    ttl: Duration,
}

impl<V> Table<V> {
    /// Evict every ready slot whose TTL has run out and drop every tombstone
    /// whose time is up. Returns the number of results evicted.
    fn sweep(&mut self, now: Duration) -> usize {
        let mut evicted = 0;
        while let Some(&Reverse((when, id))) = self.due.peek() {
            if when > now {
                break;
            }
            self.due.pop();
            match self.slots.get(&id) {
                Some(Slot::Ready { expires_at, .. }) if *expires_at <= now => {
                    self.slots.insert(id, Slot::Expired);
                    self.due.push(Reverse((now.saturating_add(self.ttl), id)));
                    evicted += 1;
                }
                Some(Slot::Expired) => {
                    self.slots.remove(&id);
                }
                // Taken already (or, impossibly, not yet ready): nothing due.
                Some(Slot::Pending { .. } | Slot::Ready { .. }) | None => {}
            }
        }
        evicted
    }

    /// Take a ready value, report a missing one, or (with `register`) add a
    /// waiter to a pending slot.
    fn lookup(&mut self, id: SlotId, register: bool) -> Lookup<V> {
        match self.slots.get_mut(&id) {
            None => Lookup::Missing(Missing::NotFound),
            Some(Slot::Pending { waiters }) => {
                if !register {
                    return Lookup::Missing(Missing::TimedOut);
                }
                // Drop the senders of waiters that gave up, so a caller that
                // polls a long task with short waits does not grow the list.
                waiters.retain(|waiter| !waiter.is_closed());
                let (tx, rx) = oneshot::channel();
                waiters.push(tx);
                Lookup::Wait(rx)
            }
            Some(Slot::Ready { .. } | Slot::Expired) => match self.slots.remove(&id) {
                Some(Slot::Ready { value, .. }) => Lookup::Taken(value),
                _ => Lookup::Missing(Missing::Expired),
            },
        }
    }
}

/// The pool's result slots.
pub(super) struct ResultSlots<V> {
    table: Mutex<Table<V>>,
}

impl<V> ResultSlots<V> {
    /// Empty slots keeping each unretrieved result for `ttl`.
    pub(super) fn new(ttl: Duration) -> Self {
        Self {
            table: Mutex::new(Table {
                slots: HashMap::new(),
                due: BinaryHeap::new(),
                ttl,
            }),
        }
    }

    /// The current TTL.
    pub(super) fn ttl(&self) -> Duration {
        self.table.lock().ttl
    }

    /// Change the TTL for results stored from now on.
    pub(super) fn set_ttl(&self, ttl: Duration) {
        self.table.lock().ttl = ttl;
    }

    /// Open a pending slot for a new task.
    pub(super) fn create(&self, id: SlotId, now: Duration) {
        let mut table = self.table.lock();
        table.sweep(now);
        table.slots.insert(
            id,
            Slot::Pending {
                waiters: Vec::new(),
            },
        );
    }

    /// Remove a slot whose task was refused at submission.
    pub(super) fn discard(&self, id: SlotId) {
        self.table.lock().slots.remove(&id);
    }

    /// Store a task's outcome and wake its waiters. Returns `false` when the
    /// slot does not exist or already holds a value (the outcome is dropped).
    pub(super) fn fill(&self, id: SlotId, value: V, now: Duration) -> bool {
        let waiters = {
            let mut table = self.table.lock();
            table.sweep(now);
            let expires_at = now.saturating_add(table.ttl);
            let waiters = match table.slots.get_mut(&id) {
                Some(Slot::Pending { waiters }) => std::mem::take(waiters),
                _ => return false,
            };
            table.slots.insert(id, Slot::Ready { value, expires_at });
            table.due.push(Reverse((expires_at, id)));
            waiters
        };
        // Wake outside the lock: a woken waiter's first act is to lock.
        for waiter in waiters {
            // A waiter that already gave up has dropped its receiver.
            let _ = waiter.send(());
        }
        true
    }

    /// Evict what is due at `now`. Returns the number of results evicted.
    pub(super) fn sweep(&self, now: Duration) -> usize {
        self.table.lock().sweep(now)
    }

    fn lookup(&self, id: SlotId, now: Duration, register: bool) -> Lookup<V> {
        let mut table = self.table.lock();
        table.sweep(now);
        table.lookup(id, register)
    }

    /// Wait up to `timeout` for slot `id` and take its value.
    ///
    /// The wait is a suspended future under a tokio timer; it holds no
    /// thread. `now` reads the pool clock for TTL sweeping. Cancel-safe:
    /// dropping the future leaves the slot (and any value) in place.
    pub(super) async fn wait(
        &self,
        id: SlotId,
        timeout: Duration,
        now: impl Fn() -> Duration,
    ) -> Result<V, Missing> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let signal = match self.lookup(id, now(), true) {
                Lookup::Taken(value) => return Ok(value),
                Lookup::Missing(missing) => return Err(missing),
                Lookup::Wait(signal) => signal,
            };
            if tokio::time::timeout_at(deadline, await_signal(signal))
                .await
                .is_err()
            {
                // The deadline passed; a value stored at that same instant is
                // still delivered.
                return self.take_now(id, now());
            }
        }
    }

    /// Wait up to `timeout` for slot `id` on the calling thread.
    ///
    /// # Panics
    ///
    /// When called on a thread that drives async tasks (inside a tokio
    /// runtime's `block_on` or a task), with tokio's "Cannot block the
    /// current thread from within a runtime" message: parking such a thread
    /// could starve the runtime. `spawn_blocking` threads and plain threads
    /// may call it.
    #[track_caller]
    pub(super) fn wait_blocking(
        &self,
        id: SlotId,
        timeout: Duration,
        now: impl Fn() -> Duration,
    ) -> Result<V, Missing> {
        refuse_async_context();
        let deadline = Instant::now() + timeout;
        loop {
            let signal = match self.lookup(id, now(), true) {
                Lookup::Taken(value) => return Ok(value),
                Lookup::Missing(missing) => return Err(missing),
                Lookup::Wait(signal) => signal,
            };
            if !park_until(signal, deadline) {
                return self.take_now(id, now());
            }
        }
    }

    /// One look without registering: the value, or why there is none.
    fn take_now(&self, id: SlotId, now: Duration) -> Result<V, Missing> {
        match self.lookup(id, now, false) {
            Lookup::Taken(value) => Ok(value),
            Lookup::Missing(missing) => Err(missing),
            Lookup::Wait(_) => Err(Missing::TimedOut),
        }
    }
}

/// Await a slot's signal. Fired and closed mean the same: look again.
#[cfg(not(parking_lot_negative_control = "s-17"))]
async fn await_signal(signal: oneshot::Receiver<()>) {
    let _ = signal.await;
}

/// Negative control S-17: wait the way the 0.2 pool did, parking a
/// blocking-pool thread per waiter. With the blocking pool exhausted the
/// waiters never run and `waiters_no_blocking_threads` must fail.
#[cfg(parking_lot_negative_control = "s-17")]
async fn await_signal(signal: oneshot::Receiver<()>) {
    let _ = tokio::task::spawn_blocking(move || signal.blocking_recv()).await;
}

/// Panic, with tokio's message, when the calling thread drives async tasks.
///
/// `blocking_recv` performs that check before it looks at the channel; on a
/// channel whose sender is already gone it then returns at once.
#[track_caller]
fn refuse_async_context() {
    let (closed, probe) = oneshot::channel::<()>();
    drop(closed);
    let _ = probe.blocking_recv();
}

/// Unparks the waiting thread when the signal fires.
struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Park the calling thread until `signal` fires or closes (`true`) or
/// `deadline` passes (`false`). A wake that arrives before the park is kept
/// by the thread's park token, so it is not lost.
fn park_until(signal: oneshot::Receiver<()>, deadline: Instant) -> bool {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut signal = pin!(signal);
    loop {
        if signal.as_mut().poll(&mut cx).is_ready() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        thread::park_timeout(deadline - now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(10);

    fn at(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn ready_value_is_taken_once() {
        let slots = ResultSlots::new(TTL);
        slots.create(1, at(0));
        assert!(slots.fill(1, "v", at(1)));
        assert_eq!(slots.take_now(1, at(2)), Ok("v"));
        assert_eq!(slots.take_now(1, at(2)), Err(Missing::NotFound));
    }

    #[test]
    fn unretrieved_value_expires_then_is_forgotten() {
        let slots = ResultSlots::new(TTL);
        slots.create(1, at(0));
        slots.fill(1, "v", at(1));
        // Kept until its TTL runs out (stored at 1s, TTL 10s).
        assert_eq!(slots.sweep(at(10)), 0);
        assert_eq!(slots.sweep(at(11)), 1);
        assert_eq!(slots.take_now(1, at(12)), Err(Missing::Expired));
        // Reported once.
        assert_eq!(slots.take_now(1, at(12)), Err(Missing::NotFound));
    }

    #[test]
    fn tombstone_is_dropped_after_another_ttl() {
        let slots = ResultSlots::new(TTL);
        slots.create(1, at(0));
        slots.fill(1, "v", at(0));
        slots.sweep(at(10));
        assert!(matches!(
            slots.table.lock().slots.get(&1),
            Some(Slot::Expired)
        ));
        slots.sweep(at(20));
        assert!(slots.table.lock().slots.is_empty());
        assert!(slots.table.lock().due.is_empty());
    }

    #[test]
    fn pending_slot_never_expires_and_reports_timed_out() {
        let slots: ResultSlots<&str> = ResultSlots::new(TTL);
        slots.create(1, at(0));
        assert_eq!(slots.sweep(at(1_000)), 0);
        assert_eq!(slots.take_now(1, at(1_000)), Err(Missing::TimedOut));
    }

    #[test]
    fn fill_without_a_slot_is_refused() {
        let slots = ResultSlots::new(TTL);
        assert!(!slots.fill(7, "v", at(0)));
        slots.create(7, at(0));
        assert!(slots.fill(7, "v", at(0)));
        assert!(!slots.fill(7, "w", at(0)));
        assert_eq!(slots.take_now(7, at(0)), Ok("v"));
    }

    #[test]
    fn gave_up_waiters_are_pruned() {
        let slots: ResultSlots<&str> = ResultSlots::new(TTL);
        slots.create(1, at(0));
        for _ in 0..100 {
            match slots.lookup(1, at(0), true) {
                Lookup::Wait(signal) => drop(signal),
                _ => unreachable!("slot is pending"),
            }
        }
        let live = match slots.table.lock().slots.get(&1) {
            Some(Slot::Pending { waiters }) => waiters.len(),
            _ => unreachable!("slot is pending"),
        };
        assert_eq!(live, 1);
    }

    #[test]
    fn blocking_wait_wakes_on_fill_from_another_thread() {
        let slots = Arc::new(ResultSlots::new(TTL));
        slots.create(1, at(0));
        let filler = Arc::clone(&slots);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            filler.fill(1, 42, at(0));
        });
        let got = slots.wait_blocking(1, Duration::from_secs(10), || at(0));
        handle.join().expect("filler");
        assert_eq!(got, Ok(42));
    }

    #[test]
    fn blocking_wait_times_out_and_keeps_the_slot() {
        let slots = ResultSlots::new(TTL);
        slots.create(1, at(0));
        let started = Instant::now();
        let got = slots.wait_blocking(1, Duration::from_millis(30), || at(0));
        assert_eq!(got, Err(Missing::TimedOut));
        assert!(started.elapsed() >= Duration::from_millis(30));
        slots.fill(1, 5, at(0));
        assert_eq!(slots.take_now(1, at(0)), Ok(5));
    }
}
