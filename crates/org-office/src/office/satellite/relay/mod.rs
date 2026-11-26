//! Cross-office native message relay: outbound journal entries and inbound relayed deltas.

mod inbound;
mod inbound_ask;
mod inbound_close;
mod inbound_handoff;
mod inbound_message;
mod outbound;
mod outbound_close;

pub use inbound::{
    RelayedAskAnswer, RelayedAskWithdrawal, RelayedMessageOutcome, RelayedRecipientWake,
};
pub use outbound::PendingMessageJournalEntry;

#[cfg(test)]
mod tests;
