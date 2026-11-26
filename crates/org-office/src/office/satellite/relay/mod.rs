//! Cross-office native message relay: outbound journal entries and inbound relayed deltas.

mod inbound;
mod inbound_ask;
mod inbound_close;
mod inbound_handoff;
mod inbound_message;
mod outbound;
mod outbound_close;
mod types;

pub use outbound::PendingMessageJournalBatch;
pub use types::{
    PendingMessageJournalEntry, RelayedAskAnswer, RelayedAskWithdrawal, RelayedMessageOutcome,
    RelayedRecipientWake,
};

#[cfg(test)]
mod tests;
