//! This office's origin streams and the feed position. Positions are recovered from the
//! journal (anchor, checkpoint, entries), never from the projection, and never wrap.

use serde::{Deserialize, Serialize};

use crate::error::StoreError;

/// The largest ordinal, origin position or feed position. Every engine and wire form can
/// hold it as a signed 64-bit integer.
pub const MAX_POSITION: u64 = i64::MAX as u64;

/// The origin streams of the data contract.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Stream {
    Organization,
    Work,
    MessageContent,
    MessageState,
}

impl Stream {
    pub const ALL: [Stream; 4] = [
        Stream::Organization,
        Stream::Work,
        Stream::MessageContent,
        Stream::MessageState,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Stream::Organization => "organization",
            Stream::Work => "work",
            Stream::MessageContent => "message_content",
            Stream::MessageState => "message_state",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stream| stream.name() == name)
    }
}

/// The last position taken in each origin stream. The organization stream's high-water is
/// the directory generation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighWaters {
    pub organization: u64,
    pub work: u64,
    pub message_content: u64,
    pub message_state: u64,
}

impl HighWaters {
    pub fn get(&self, stream: Stream) -> u64 {
        match stream {
            Stream::Organization => self.organization,
            Stream::Work => self.work,
            Stream::MessageContent => self.message_content,
            Stream::MessageState => self.message_state,
        }
    }

    pub fn set(&mut self, stream: Stream, value: u64) {
        match stream {
            Stream::Organization => self.organization = value,
            Stream::Work => self.work = value,
            Stream::MessageContent => self.message_content = value,
            Stream::MessageState => self.message_state = value,
        }
    }

    /// No stream of `self` is behind the same stream of `earlier`.
    pub fn none_behind(&self, earlier: &HighWaters) -> bool {
        Stream::ALL
            .into_iter()
            .all(|stream| self.get(stream) >= earlier.get(stream))
    }

    pub fn valid(&self) -> bool {
        Stream::ALL
            .into_iter()
            .all(|stream| self.get(stream) <= MAX_POSITION)
    }
}

/// A position in the store's change feed: one per journal entry, never reused.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct FeedPosition(pub u64);

/// The allocator. The writer holds the only live one; a batch works on a copy and keeps it
/// only once its entries are appended.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sequences {
    pub high_waters: HighWaters,
    pub feed: FeedPosition,
}

impl Sequences {
    /// The next position of `stream`. Exhaustion is an error, never a wrap.
    pub fn next(&mut self, stream: Stream) -> Result<u64, StoreError> {
        let next = successor(self.high_waters.get(stream), stream.name())?;
        self.high_waters.set(stream, next);
        Ok(next)
    }

    pub fn next_feed(&mut self) -> Result<FeedPosition, StoreError> {
        self.feed = FeedPosition(successor(self.feed.0, "feed position")?);
        Ok(self.feed)
    }
}

pub(crate) fn successor(value: u64, what: &'static str) -> Result<u64, StoreError> {
    if value >= MAX_POSITION {
        return Err(StoreError::Exhausted(what));
    }
    Ok(value + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_never_wrap_and_streams_are_independent() {
        let mut sequences = Sequences::default();
        assert_eq!(sequences.next(Stream::Work).unwrap(), 1);
        assert_eq!(sequences.next(Stream::Work).unwrap(), 2);
        assert_eq!(sequences.next(Stream::Organization).unwrap(), 1);
        assert_eq!(sequences.high_waters.message_content, 0);

        sequences.high_waters.work = MAX_POSITION - 1;
        assert_eq!(sequences.next(Stream::Work).unwrap(), MAX_POSITION);
        assert_eq!(
            sequences.next(Stream::Work),
            Err(StoreError::Exhausted("work"))
        );
        assert_eq!(sequences.high_waters.work, MAX_POSITION);

        sequences.feed = FeedPosition(MAX_POSITION);
        assert_eq!(
            sequences.next_feed(),
            Err(StoreError::Exhausted("feed position"))
        );
        assert_eq!(sequences.feed, FeedPosition(MAX_POSITION));
    }

    #[test]
    fn stream_names_are_the_data_contracts_and_round_trip() {
        let names: Vec<_> = Stream::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(
            names,
            ["organization", "work", "message_content", "message_state"]
        );
        for stream in Stream::ALL {
            assert_eq!(Stream::from_name(stream.name()), Some(stream));
        }
        assert_eq!(Stream::from_name("message"), None);
        let json = serde_json::to_string(&HighWaters::default()).unwrap();
        assert_eq!(
            json,
            r#"{"organization":0,"work":0,"message_content":0,"message_state":0}"#
        );
    }
}
