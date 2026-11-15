//! Checked PHC ownership, with no password hashing or registry dependency.
use super::Error;
use argon2::password_hash::PasswordHash;

/// A pinned Argon2id PHC. Deliberately neither Debug nor Clone.
pub(crate) struct Verifier(String);
impl Verifier {
    pub(crate) fn new(phc: String) -> Result<Self, Error> {
        // Own the input before any fallible parsing so rejection also zeroizes it.
        let verifier = Self(phc);
        let hash = PasswordHash::new(verifier.expose()).map_err(|_| Error::Unsafe)?;
        if hash.algorithm.as_str() != "argon2id"
            || hash.version != Some(19)
            || hash.params.iter().count() != 3
            || hash.params.get_decimal("m") != Some(65_536)
            || hash.params.get_decimal("t") != Some(3)
            || hash.params.get_decimal("p") != Some(1)
            || hash.hash.as_ref().map(|tag| tag.len()) != Some(32)
        {
            return Err(Error::Unsafe);
        }
        let mut salt = [0; 16];
        if hash
            .salt
            .ok_or(Error::Unsafe)?
            .decode_b64(&mut salt)
            .map_err(|_| Error::Unsafe)?
            .len()
            != 16
        {
            return Err(Error::Unsafe);
        }
        Ok(verifier)
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl Drop for Verifier {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        crate::secrets::zero(&mut bytes);
        #[cfg(test)]
        tests::observe_clear(&bytes);
    }
}

#[cfg(test)]
mod tests;
