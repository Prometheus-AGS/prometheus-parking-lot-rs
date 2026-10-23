//! Result slots with async notification, without a runtime.
//!
//! One slot per submitted task. A retriever claims the slot: it takes a
//! finished outcome at once, or parks a `futures-channel` oneshot in the slot
//! and awaits it; the worker that stores the outcome fires the oneshot. No
//! polling, no tokio.

use std::collections::HashMap;

use futures_channel::oneshot;
use parking_lot::Mutex;

/// A task's result slot.
struct Slot<T> {
    /// The outcome, once the task has finished.
    outcome: Option<T>,
    /// The retriever waiting for the outcome, if any.
    waiter: Option<oneshot::Sender<()>>,
}

/// What a retriever found in the slot.
pub enum Claim<T> {
    /// The task had finished; the outcome is taken and the slot removed.
    Ready(T),
    /// The task is still running; await the receiver, then [`Results::remove`].
    Wait(oneshot::Receiver<()>),
    /// No such slot (never created, already retrieved, or timed out).
    Missing,
}

/// Result slots keyed by the task's mailbox key string.
pub struct Results<T> {
    slots: Mutex<HashMap<String, Slot<T>>>,
}

impl<T> Results<T> {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// Create the slot for a task about to be submitted.
    pub fn create(&self, key: &str) {
        self.slots.lock().insert(
            key.to_owned(),
            Slot {
                outcome: None,
                waiter: None,
            },
        );
    }

    /// Store the outcome and wake the waiting retriever. A slot that was
    /// removed (the retriever timed out) drops the outcome.
    pub fn store(&self, key: &str, outcome: T) {
        let waiter = self.slots.lock().get_mut(key).and_then(|slot| {
            slot.outcome = Some(outcome);
            slot.waiter.take()
        });
        if let Some(waiter) = waiter {
            // The receiver may be gone (the retriever timed out): fine.
            let _ = waiter.send(());
        }
    }

    /// Take the outcome if it is there, otherwise register as the waiter.
    pub fn claim(&self, key: &str) -> Claim<T> {
        let mut slots = self.slots.lock();
        let finished = match slots.get_mut(key) {
            None => return Claim::Missing,
            Some(slot) if slot.outcome.is_none() => {
                let (tx, rx) = oneshot::channel();
                slot.waiter = Some(tx);
                return Claim::Wait(rx);
            }
            Some(_) => slots.remove(key),
        };
        drop(slots);
        finished
            .and_then(|slot| slot.outcome)
            .map_or(Claim::Missing, Claim::Ready)
    }

    /// Remove the slot, returning the outcome if it had arrived.
    pub fn remove(&self, key: &str) -> Option<T> {
        self.slots.lock().remove(key).and_then(|slot| slot.outcome)
    }
}
