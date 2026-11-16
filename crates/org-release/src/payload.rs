use crate::Error;
use sha2::{Digest, Sha256};

/// Incremental exact-size/SHA256 check, constructed only from a verified entry.
/// Memory use is constant regardless of asset size. An overrun permanently
/// poisons the verifier; discarding an error cannot permit a successful finish.
#[derive(Debug)]
pub struct PayloadVerifier {
    expected_size: u64,
    expected_sha256: [u8; 32],
    received: u64,
    hasher: Sha256,
    overrun: bool,
}

impl PayloadVerifier {
    pub(crate) fn new(expected_size: u64, expected_sha256: [u8; 32]) -> Self {
        Self {
            expected_size,
            expected_sha256,
            received: 0,
            hasher: Sha256::new(),
            overrun: false,
        }
    }

    /// Add bytes in their original order. Refuse overruns before hashing them.
    pub fn update(&mut self, chunk: &[u8]) -> Result<(), Error> {
        let next = u64::try_from(chunk.len())
            .ok()
            .and_then(|length| self.received.checked_add(length));
        if self.overrun || next.is_none_or(|size| size > self.expected_size) {
            self.overrun = true;
            return Err(Error::PayloadOverrun);
        }
        self.received = next.ok_or(Error::PayloadOverrun)?;
        self.hasher.update(chunk);
        Ok(())
    }

    pub fn bytes_received(&self) -> u64 {
        self.received
    }

    /// Consume the verifier only after EOF. The caller must establish that EOF
    /// belongs to the same held input/descriptor it intended to validate.
    pub fn finish(self) -> Result<(), Error> {
        if self.overrun {
            return Err(Error::PayloadOverrun);
        }
        if self.received != self.expected_size {
            return Err(Error::PayloadUnderrun);
        }
        if <[u8; 32]>::from(self.hasher.finalize()) != self.expected_sha256 {
            return Err(Error::PayloadDigestMismatch);
        }
        Ok(())
    }
}
