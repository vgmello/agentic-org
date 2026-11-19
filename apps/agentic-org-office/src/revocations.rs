//! Bounded, atomic session tombstones. The transfer service is the sole writer.
use std::collections::HashSet;
use std::io;
#[derive(Default)]
pub(crate) struct Revocations(HashSet<String>);
impl Revocations {
    pub fn contains(&self, session: &str) -> bool {
        self.0.contains(session)
    }

    // Every worker restart rotates its grant key. Retain until exit, without eviction.
    pub fn apply_batch(&mut self, ids: Vec<String>) -> io::Result<()> {
        let incoming: HashSet<_> = ids.into_iter().collect();
        if self.0.len() + incoming.difference(&self.0).count() > 4096 {
            return Err(io::Error::other("revocation capacity exceeded"));
        }
        self.0.extend(incoming);
        Ok(())
    }
}
#[cfg(test)]
mod tests;
