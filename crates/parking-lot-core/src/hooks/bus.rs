//! [`HookBus`]: off-path, ordered delivery of [`LifecycleEvent`]s to hooks.
//!
//! ## Delivery
//!
//! A pool emits on its scheduling path (submission, the dispatch lock,
//! completion); a hook is user code that may be slow, block on I/O or panic.
//! The bus keeps the two apart:
//!
//! - **Emission never blocks and never fails.** An event goes into a bounded
//!   FIFO channel with `try_send`. When the channel is full (a hook that cannot
//!   keep up), the event is dropped and counted ([`HookBus::dropped_events`]);
//!   the scheduling path does not wait.
//! - **One delivery thread per bus** (`pl-hooks`), started when the first hook
//!   is registered, calls every hook for every event in channel order. Hooks
//!   are synchronous and may block, so they run on a thread of their own rather
//!   than on an async runtime's workers. Each call is wrapped in
//!   `catch_unwind`: a panicking hook is counted ([`HookBus::hook_panics`]) and
//!   logged, and delivery continues. The thread exits once every handle to the
//!   bus is dropped and the channel is drained.
//! - **Order.** The channel is a linearizable FIFO, so if emission A
//!   happens-before emission B (same thread, or ordered through a lock or a
//!   spawn), every hook sees A before B. The pools emit each task's events
//!   along such edges, which is what makes "submitted before started before
//!   one terminal event" hold for every observer.
//! - **Zero overhead without hooks.** Until a hook is registered there is no
//!   channel and no thread: [`HookBus::emit_with`] is one atomic load and a
//!   branch, and the event (its `String`s included) is never built.
//!
//! **On `wasm32` targets, which have no threads, none of the isolation
//! above holds.** Delivery is inline on the emitting call: a slow hook
//! stalls the pool's scheduling path; the `Queued` event is delivered while
//! the pool holds its dispatch lock, so a hook must not call back into the
//! pool (it would deadlock or panic on the lock); and the `catch_unwind`
//! around each call contains nothing on `wasm32-unknown-unknown`, which
//! aborts on panic, so a panicking hook aborts the module. Keep wasm hooks
//! short, non-panicking and non-reentrant.
//!
//! The uncomfortable part: under sustained hook backpressure events are
//! dropped, so an observer can miss a task's terminal event. That is the price
//! of never stalling dispatch; `dropped_events` makes it visible, and a hook
//! that must not lose events should hand them to its own unbounded store
//! quickly rather than doing the slow work inline.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::{Mutex, RwLock};

use super::{Hook, LifecycleEvent};

/// Events that can wait for the delivery thread before new ones are dropped.
pub const DEFAULT_HOOK_QUEUE_CAPACITY: usize = 4096;

/// The registered hooks and the delivery-side counters, shared by the bus
/// handles and the delivery thread (which must not keep the bus itself alive,
/// or its channel would never close).
struct Observers {
    /// Snapshot-on-write: delivery clones the `Arc` and calls hooks without
    /// holding the lock, so a hook may register another hook.
    hooks: RwLock<Arc<[Arc<dyn Hook>]>>,
    delivered: AtomicU64,
    panicked: AtomicU64,
}

impl Observers {
    /// Call every hook with `event`, containing panics.
    fn deliver(&self, event: &LifecycleEvent) {
        let hooks = Arc::clone(&self.hooks.read());
        for hook in hooks.iter() {
            if catch_unwind(AssertUnwindSafe(|| hook.on_event(event))).is_err() {
                // Relaxed: a statistic.
                self.panicked.fetch_add(1, Ordering::Relaxed);
                tracing::error!(
                    kind = event.kind(),
                    task_id = event.task_id(),
                    "lifecycle hook panicked; delivery continues"
                );
            }
        }
        // Relaxed: a statistic.
        self.delivered.fetch_add(1, Ordering::Relaxed);
    }
}

/// What the delivery thread receives.
#[cfg(not(target_arch = "wasm32"))]
enum Message {
    Event(LifecycleEvent),
    /// Acknowledge once every earlier message has been delivered.
    Flush(std::sync::mpsc::SyncSender<()>),
}

/// How emitted events reach the hooks; set once, when the first hook is
/// registered.
enum Delivery {
    #[cfg(not(target_arch = "wasm32"))]
    Thread(std::sync::mpsc::SyncSender<Message>),
    #[cfg(target_arch = "wasm32")]
    Inline,
}

struct Shared {
    capacity: usize,
    observers: Arc<Observers>,
    /// `None` until a hook is registered. Emission reads it with one
    /// acquire load (`OnceLock::get`).
    delivery: OnceLock<Delivery>,
    /// Serializes starting the delivery thread.
    starting: Mutex<()>,
    dropped: AtomicU64,
}

/// A registry of [`Hook`] observers for one pool, with off-path delivery.
///
/// A cheap handle: clones share the hooks and the delivery thread, so one bus
/// can serve several pools. See the module docs (`hooks::bus`) for the
/// delivery guarantees. Two handles are equal when they are the same bus.
///
/// On `wasm32` delivery is inline, on the pool's scheduling path (under its
/// dispatch lock for `Queued`), and a hook panic aborts the module; the
/// module docs give the details.
#[derive(Clone)]
pub struct HookBus {
    shared: Arc<Shared>,
}

impl Default for HookBus {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_HOOK_QUEUE_CAPACITY)
    }
}

impl fmt::Debug for HookBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookBus")
            .field("hooks", &self.shared.observers.hooks.read().len())
            .field("capacity", &self.shared.capacity)
            .field("dropped_events", &self.dropped_events())
            .finish()
    }
}

impl PartialEq for HookBus {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Eq for HookBus {}

impl HookBus {
    /// An empty bus whose channel holds [`DEFAULT_HOOK_QUEUE_CAPACITY`]
    /// undelivered events.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty bus whose channel holds `capacity` undelivered events (at
    /// least one).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                capacity: capacity.max(1),
                observers: Arc::new(Observers {
                    hooks: RwLock::new(Arc::from(Vec::new())),
                    delivered: AtomicU64::new(0),
                    panicked: AtomicU64::new(0),
                }),
                delivery: OnceLock::new(),
                starting: Mutex::new(()),
                dropped: AtomicU64::new(0),
            }),
        }
    }

    /// Register a hook. It receives every event emitted from now on (events
    /// already in the channel included). The first registration starts the
    /// delivery thread; if the thread cannot be spawned the error is logged
    /// and the bus stays inactive.
    pub fn register(&self, hook: Arc<dyn Hook>) {
        {
            let mut hooks = self.shared.observers.hooks.write();
            let mut next = hooks.to_vec();
            next.push(hook);
            *hooks = Arc::from(next);
        }
        self.start_delivery();
    }

    /// Returns `true` once a hook is registered (and delivery started).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.shared.delivery.get().is_some()
    }

    /// Emit the event `make` builds, if any hook is registered.
    ///
    /// Without hooks this is one atomic load and a branch: `make` is not
    /// called, so nothing is allocated. With hooks the event is queued with
    /// a non-blocking send; a full queue drops it (counted in
    /// [`dropped_events`](Self::dropped_events)). Never blocks, never
    /// panics on a hook's behalf, so it is safe under a pool's locks. On
    /// `wasm32` it calls the hooks inline instead (see the module docs).
    #[inline]
    pub fn emit_with(&self, make: impl FnOnce() -> LifecycleEvent) {
        // `OnceLock::get` is an acquire load: it synchronizes with the
        // release in `OnceLock::set` (`start_delivery`), so the channel the
        // registering thread created is fully visible here.
        if let Some(delivery) = self.shared.delivery.get() {
            #[cfg(not(target_arch = "wasm32"))]
            self.send(delivery, make());
            #[cfg(target_arch = "wasm32")]
            self.send(delivery, &make());
        }
    }

    /// Emit a copy of `event` to all registered hooks. No-op (no clone)
    /// when none is registered; see [`emit_with`](Self::emit_with).
    pub fn emit(&self, event: &LifecycleEvent) {
        self.emit_with(|| event.clone());
    }

    /// Wait until every event emitted before this call has been delivered,
    /// or `timeout` passes. Returns `true` when delivered (and at once when
    /// no hook is registered).
    ///
    /// Blocks the calling thread: call it from tests, shutdown code or
    /// `spawn_blocking`, not from latency-sensitive async code.
    #[must_use]
    pub fn flush(&self, timeout: std::time::Duration) -> bool {
        match self.shared.delivery.get() {
            None => true,
            #[cfg(not(target_arch = "wasm32"))]
            Some(Delivery::Thread(sender)) => flush_thread(sender, timeout),
            #[cfg(target_arch = "wasm32")]
            Some(Delivery::Inline) => {
                let _ = timeout;
                true
            }
        }
    }

    /// Events dropped because the delivery queue was full (or delivery had
    /// stopped).
    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        // Relaxed: a statistic.
        self.shared.dropped.load(Ordering::Relaxed)
    }

    /// Events delivered to every registered hook so far.
    #[must_use]
    pub fn delivered_events(&self) -> u64 {
        // Relaxed: a statistic.
        self.shared.observers.delivered.load(Ordering::Relaxed)
    }

    /// Hook calls that panicked (each was contained; delivery continued).
    #[must_use]
    pub fn hook_panics(&self) -> u64 {
        // Relaxed: a statistic.
        self.shared.observers.panicked.load(Ordering::Relaxed)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn send(&self, delivery: &Delivery, event: LifecycleEvent) {
        let Delivery::Thread(sender) = delivery;
        if sender.try_send(Message::Event(event)).is_err() {
            self.note_dropped();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn send(&self, delivery: &Delivery, event: &LifecycleEvent) {
        let Delivery::Inline = delivery;
        self.shared.observers.deliver(event);
    }

    /// Count a dropped event; warn on the first and every power of two after.
    #[cfg(not(target_arch = "wasm32"))]
    fn note_dropped(&self) {
        // Relaxed: a statistic.
        let before = self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        if before == 0 || (before + 1).is_power_of_two() {
            tracing::warn!(
                dropped = before + 1,
                capacity = self.shared.capacity,
                "lifecycle hook queue full; events dropped (a hook is not keeping up)"
            );
        }
    }

    fn start_delivery(&self) {
        if self.shared.delivery.get().is_some() {
            return;
        }
        let _starting = self.shared.starting.lock();
        if self.shared.delivery.get().is_some() {
            return;
        }
        match spawn_delivery(self.shared.capacity, &self.shared.observers) {
            Ok(delivery) => {
                // Release: see `emit_with`. Only this thread sets it (under
                // `starting`), so the set cannot fail.
                let _ = self.shared.delivery.set(delivery);
            }
            Err(error) => {
                tracing::error!(%error, "could not start the lifecycle hook thread; hooks receive no events");
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_delivery(capacity: usize, observers: &Arc<Observers>) -> std::io::Result<Delivery> {
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Message>(capacity);
    let observers = Arc::clone(observers);
    std::thread::Builder::new()
        .name("pl-hooks".into())
        .spawn(move || {
            // Ends when every sender (every bus handle) is gone and the
            // channel is drained.
            while let Ok(message) = receiver.recv() {
                match message {
                    Message::Event(event) => observers.deliver(&event),
                    Message::Flush(ack) => {
                        let _ = ack.send(());
                    }
                }
            }
        })?;
    Ok(Delivery::Thread(sender))
}

#[cfg(target_arch = "wasm32")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "same signature as the threaded variant"
)]
const fn spawn_delivery(
    _capacity: usize,
    _observers: &Arc<Observers>,
) -> std::io::Result<Delivery> {
    Ok(Delivery::Inline)
}

/// Queue a flush marker behind everything emitted so far and wait for the
/// delivery thread to reach it.
#[cfg(not(target_arch = "wasm32"))]
fn flush_thread(
    sender: &std::sync::mpsc::SyncSender<Message>,
    timeout: std::time::Duration,
) -> bool {
    use std::sync::mpsc::TrySendError;
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + timeout;
    let (ack, acked) = std::sync::mpsc::sync_channel(1);
    let mut marker = Message::Flush(ack);
    loop {
        match sender.try_send(marker) {
            Ok(()) => break,
            Err(TrySendError::Full(back)) => {
                if Instant::now() >= deadline {
                    return false;
                }
                marker = back;
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
    acked
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_ok()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::hooks::RecordingHook;

    fn submitted(task_id: u64) -> LifecycleEvent {
        LifecycleEvent::Submitted {
            task_id,
            idempotency_key: None,
            pool: "p".into(),
            at_ms: 0,
        }
    }

    struct Panicky;
    impl Hook for Panicky {
        fn on_event(&self, _event: &LifecycleEvent) {
            panic!("hook panics on purpose");
        }
    }

    /// Blocks until released, so the queue fills behind it.
    struct Gate(std::sync::Mutex<()>);
    impl Hook for Gate {
        fn on_event(&self, _event: &LifecycleEvent) {
            drop(self.0.lock());
        }
    }

    #[test]
    fn inactive_bus_never_builds_the_event() {
        let bus = HookBus::new();
        bus.emit_with(|| unreachable!("no hook registered"));
        assert!(!bus.is_active());
        assert!(bus.flush(Duration::from_millis(1)));
    }

    #[test]
    fn panicking_hook_does_not_stop_delivery() {
        let bus = HookBus::new();
        let recorder = Arc::new(RecordingHook::new(16));
        bus.register(Arc::new(Panicky));
        bus.register(Arc::clone(&recorder) as Arc<dyn Hook>);
        for id in 0..3 {
            bus.emit(&submitted(id));
        }
        assert!(bus.flush(Duration::from_secs(5)));
        assert_eq!(recorder.len(), 3);
        assert_eq!(bus.hook_panics(), 3);
    }

    #[test]
    fn blocked_hook_drops_events_instead_of_blocking_emitters() {
        let bus = HookBus::with_capacity(2);
        let gate = Arc::new(Gate(std::sync::Mutex::new(())));
        let held = gate.0.lock().expect("gate");
        bus.register(Arc::clone(&gate) as Arc<dyn Hook>);
        for id in 0..10 {
            bus.emit(&submitted(id));
        }
        assert!(bus.dropped_events() >= 7, "{bus:?}");
        drop(held);
        assert!(bus.flush(Duration::from_secs(5)));
    }
}
