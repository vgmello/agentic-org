//! Framing and reading back one journal entry, and the journal points recovered from entries.

use std::collections::BTreeMap;

use super::{Entry, EntryDraft, FileRecord, Header, JournalPoint, MAX_ENTRY_BYTES, TRAILER};
use crate::change::Sha256;
use crate::custody::{JournalHead, OfficeIdentity};
use crate::engine::Cursor;
use crate::error::JournalError;
use crate::files::{FileOp, FileVersion};
use crate::sequences::{HighWaters, Stream, MAX_POSITION};

impl Entry {
    pub fn frame(draft: EntryDraft) -> Result<Entry, JournalError> {
        let mut files = Vec::with_capacity(draft.files.len());
        for file in &draft.files {
            if !file.path.names_a_file() {
                return Err(JournalError::Internal(format!(
                    "{} is a tree, not a file",
                    file.path
                )));
            }
            if file.bytes.is_none() != (file.op == FileOp::Remove) {
                return Err(JournalError::Internal(
                    "a removal has no bytes and every other change has".into(),
                ));
            }
            files.push(FileRecord {
                path: file.path.clone(),
                op: file.op,
                length: file.bytes.as_ref().map_or(0, |b| b.len() as u64),
                sha256: file.bytes.as_ref().map(|b| Sha256::of(b)),
                previous_sha256: file.previous_sha256,
            });
        }
        let header = Header {
            ordinal: draft.ordinal,
            previous_sha256: draft.previous_sha256,
            office_id: draft.office_id,
            journal_incarnation: draft.journal_incarnation,
            at: draft.at,
            ack: draft.ack,
            feed: draft.feed,
            operation: draft.operation,
            positions: draft.positions,
            files,
        };
        let head =
            serde_json::to_vec(&header).map_err(|e| JournalError::Internal(e.to_string()))?;
        let body = head.len()
            + header
                .files
                .iter()
                .map(|f| f.length as usize)
                .sum::<usize>();
        let total = 4 + body + TRAILER;
        if total > MAX_ENTRY_BYTES {
            return Err(JournalError::TooLarge { bytes: total });
        }
        let mut frame = Vec::with_capacity(total);
        frame.extend_from_slice(&(body as u32).to_be_bytes());
        frame.extend_from_slice(&head);
        let mut payloads = Vec::with_capacity(draft.files.len());
        for file in &draft.files {
            let start = frame.len();
            if let Some(bytes) = &file.bytes {
                frame.extend_from_slice(bytes);
            }
            payloads.push(start..frame.len());
        }
        let sha256 = Sha256::of(&frame[4..]);
        frame.extend_from_slice(sha256.as_bytes());
        Ok(Entry {
            header,
            sha256,
            frame: frame.into(),
            payloads,
        })
    }

    /// Reads one whole frame whose length and hash are already verified.
    pub(super) fn parse(frame: &[u8], sha256: Sha256) -> Result<Entry, String> {
        let body = &frame[4..frame.len() - TRAILER];
        let mut stream = serde_json::Deserializer::from_slice(body).into_iter::<Header>();
        let header = match stream.next() {
            Some(Ok(header)) => header,
            Some(Err(e)) => return Err(format!("unreadable header: {e}")),
            None => return Err("no header".into()),
        };
        let mut at = 4 + stream.byte_offset();
        let mut payloads = Vec::with_capacity(header.files.len());
        for file in &header.files {
            if (file.op == FileOp::Remove) != file.sha256.is_none()
                || (file.op == FileOp::Remove && file.length != 0)
            {
                return Err(format!(
                    "{} has the wrong shape for its operation",
                    file.path
                ));
            }
            let end = (at as u64)
                .checked_add(file.length)
                .filter(|end| *end <= (frame.len() - TRAILER) as u64)
                .ok_or("a file runs past the entry")? as usize;
            payloads.push(at..end);
            at = end;
        }
        if at != frame.len() - TRAILER {
            return Err("the entry holds bytes no file accounts for".into());
        }
        Ok(Entry {
            header,
            sha256,
            frame: frame.into(),
            payloads,
        })
    }

    pub fn ordinal(&self) -> u64 {
        self.header.ordinal
    }

    /// The stored bytes of the whole frame.
    pub fn stored(&self) -> &[u8] {
        &self.frame
    }

    /// The bytes of file `index`; empty for a removal.
    pub fn payload(&self, index: usize) -> &[u8] {
        &self.frame[self.payloads[index].clone()]
    }

    /// The entry's files as versions to materialize, in order.
    pub fn versions(&self) -> impl Iterator<Item = FileVersion<'_>> {
        self.header
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| FileVersion {
                path: &file.path,
                op: file.op,
                bytes: (file.op != FileOp::Remove).then(|| self.payload(i)),
                sha256: file.sha256,
                previous_sha256: file.previous_sha256,
            })
    }
}

/// The positions an operation took, as an entry records them.
pub fn positions_between(before: &HighWaters, after: &HighWaters) -> BTreeMap<String, [u64; 2]> {
    Stream::ALL
        .into_iter()
        .filter(|stream| after.get(*stream) > before.get(*stream))
        .map(|stream| {
            (
                stream.name().to_owned(),
                [before.get(stream) + 1, after.get(stream)],
            )
        })
        .collect()
}

impl JournalPoint {
    pub fn head(&self) -> JournalHead {
        JournalHead {
            ordinal: self.ordinal,
            sha256: self.sha256,
            high_waters: self.high_waters,
        }
    }

    pub fn cursor(&self) -> Cursor {
        Cursor {
            ordinal: self.ordinal,
            sha256: self.sha256,
            feed: self.feed,
            high_waters: self.high_waters,
        }
    }

    /// The point after `entry`, or why `entry` does not continue the chain from here.
    pub(super) fn after(
        &self,
        entry: &Entry,
        identity: &OfficeIdentity,
    ) -> Result<JournalPoint, String> {
        let header = &entry.header;
        if header.ordinal != self.ordinal + 1 {
            return Err(format!(
                "ordinal {} follows {}",
                header.ordinal, self.ordinal
            ));
        }
        if header.previous_sha256 != self.sha256 {
            return Err("it does not chain to its predecessor".into());
        }
        if header.office_id != identity.office_id
            || header.journal_incarnation != identity.journal_incarnation
        {
            return Err("it belongs to another journal".into());
        }
        if header.feed.0 != self.feed.0 + 1 || header.feed.0 > MAX_POSITION {
            return Err("its feed position does not follow".into());
        }
        let mut high_waters = self.high_waters;
        for (name, [first, last]) in &header.positions {
            let stream = Stream::from_name(name).ok_or("an unknown origin stream")?;
            if *first != high_waters.get(stream) + 1 || last < first || *last > MAX_POSITION {
                return Err(format!("its {name} positions do not follow"));
            }
            high_waters.set(stream, *last);
        }
        Ok(JournalPoint {
            ordinal: header.ordinal,
            sha256: entry.sha256,
            high_waters,
            feed: header.feed,
        })
    }
}
