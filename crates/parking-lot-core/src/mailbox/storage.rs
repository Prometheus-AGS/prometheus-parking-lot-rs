//! [`ResultMailbox`] behaviour of [`StorageMailbox`]: store first, then notify.
//!
//! `deliver` stores in the storage backend. Only a [`DeliveryOutcome::Stored`]
//! result triggers a notification (a `Duplicate` stored nothing, so there is
//! nothing new to announce), and the notification is handed to the notifier's
//! [`DetachedNotifier::dispatch`], which returns immediately. The delivery's
//! result is the storage's result and cannot depend on the notifier. The other
//! methods go straight to storage.
//!
//! Under the test-only `--cfg parking_lot_negative_control="s-10"`, `deliver`
//! instead notifies in line before storing and aborts on a notifier error,
//! which loses the result when the notifier is down.

use std::future::Future;

use crate::contracts::error::SchedulerError;
use crate::contracts::result_mailbox::{
    Delivery, DeliveryOutcome, MailboxEntry, ResultMailbox, StorageMailbox,
};
use crate::contracts::types::MailboxKey;
use crate::mailbox::notify::DetachedNotifier;

impl<T, S, N> ResultMailbox<T> for StorageMailbox<S, N>
where
    T: Clone + Send + Sync + 'static,
    S: ResultMailbox<T>,
    N: DetachedNotifier<T>,
{
    #[cfg(not(parking_lot_negative_control = "s-10"))]
    async fn deliver(&self, delivery: Delivery<T>) -> Result<DeliveryOutcome, SchedulerError> {
        let copy = N::ACTIVE.then(|| delivery.clone());
        let outcome = self.storage().deliver(delivery).await?;
        if outcome == DeliveryOutcome::Stored
            && let Some(copy) = &copy
        {
            self.notifier().dispatch(copy);
        }
        Ok(outcome)
    }

    #[cfg(parking_lot_negative_control = "s-10")]
    async fn deliver(&self, delivery: Delivery<T>) -> Result<DeliveryOutcome, SchedulerError> {
        self.notifier().notify_inline(&delivery).await?;
        self.storage().deliver(delivery).await
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send {
        self.storage().consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send {
        self.storage().peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.storage().evict_expired()
    }
}
