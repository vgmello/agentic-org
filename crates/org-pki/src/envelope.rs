use super::{Code, Error, SigningKey};
use argon2::{Algorithm, Argon2, Block, Params, Version};
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use zeroize::Zeroizing;
pub const MAX_PACKAGE_BYTES: usize = 4475;
const MAGIC: &[u8; 8] = b"AGORGCA1";
const DOMAIN: &[u8] = b"Agentic Org CA escrow v1\0";
pub struct Envelope<'a> {
    header: &'a [u8],
    organization: &'a str,
    salt: &'a [u8],
    nonce: [u8; 12],
    public: [u8; 65],
    ciphertext: &'a [u8],
}
impl<'a> Envelope<'a> {
    pub fn parse(package: &'a [u8], organization: &str) -> Result<Self, Error> {
        if package.len() > MAX_PACKAGE_BYTES || package.len() < 10 || &package[..8] != MAGIC {
            return Err(Error::InvalidPackage);
        }
        let length = u16::from_be_bytes(
            package[8..10]
                .try_into()
                .map_err(|_| Error::InvalidPackage)?,
        ) as usize;
        if !(1..=255).contains(&length) {
            return Err(Error::InvalidPackage);
        }
        let header_len = 10 + length + 1 + 16 + 12 + 65 + 4;
        if package.len() < header_len {
            return Err(Error::InvalidPackage);
        }
        let recorded =
            std::str::from_utf8(&package[10..10 + length]).map_err(|_| Error::InvalidPackage)?;
        if recorded != organization || package[10 + length] != 1 {
            return Err(Error::InvalidPackage);
        }
        let salt_start = 11 + length;
        let nonce_start = salt_start + 16;
        let public_start = nonce_start + 12;
        let public: [u8; 65] = package[public_start..public_start + 65]
            .try_into()
            .map_err(|_| Error::InvalidPackage)?;
        if public[0] != 4 {
            return Err(Error::InvalidPackage);
        }
        let ciphertext_len = u32::from_be_bytes(
            package[header_len - 4..header_len]
                .try_into()
                .map_err(|_| Error::InvalidPackage)?,
        ) as usize;
        if !(17..=4112).contains(&ciphertext_len) || package.len() != header_len + ciphertext_len {
            return Err(Error::InvalidPackage);
        }
        Ok(Self {
            header: &package[..header_len],
            organization: recorded,
            salt: &package[salt_start..nonce_start],
            nonce: package[nonce_start..public_start]
                .try_into()
                .map_err(|_| Error::InvalidPackage)?,
            public,
            ciphertext: &package[header_len..],
        })
    }
    pub fn organization(&self) -> &str {
        self.organization
    }
    pub fn public_key(&self) -> &[u8; 65] {
        &self.public
    }
    pub fn recover(&self, code: &Code) -> Result<SigningKey, Error> {
        let key = derive(code, self.salt)?;
        let key = LessSafeKey::new(
            UnboundKey::new(&aead::AES_256_GCM, &key[..]).map_err(|_| Error::Unavailable)?,
        );
        let aad = aad(self.header);
        let mut plaintext = Zeroizing::new(self.ciphertext.to_vec());
        let decoded = key
            .open_in_place(
                Nonce::assume_unique_for_key(self.nonce),
                Aad::from(aad),
                &mut plaintext,
            )
            .map_err(|_| Error::RecoveryFailed)?;
        let key = SigningKey::from_local_der(decoded).map_err(|_| Error::RecoveryFailed)?;
        if key.public_key() != self.public {
            return Err(Error::RecoveryFailed);
        }
        Ok(key)
    }
}
fn aad(header: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(DOMAIN.len() + header.len());
    aad.extend_from_slice(DOMAIN);
    aad.extend_from_slice(header);
    aad
}
fn derive(code: &Code, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, Error> {
    #[cfg(test)]
    KDFS.with(|count| count.set(count.get() + 1));
    let params = Params::new(65536, 3, 4, Some(32)).map_err(|_| Error::Unavailable)?;
    let mut blocks = Zeroizing::new(Vec::new());
    blocks
        .try_reserve_exact(params.block_count())
        .map_err(|_| Error::Unavailable)?;
    blocks.resize(params.block_count(), Block::default());
    let mut key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(&code.0[..], salt, &mut *key, &mut *blocks)
        .map_err(|_| Error::Unavailable)?;
    Ok(key)
}
pub(super) fn seal(key: &SigningKey, organization: &str, code: &Code) -> Result<Vec<u8>, Error> {
    if organization.is_empty() || organization.len() > 255 {
        return Err(Error::InvalidPackage);
    }
    let mut salt = [0; 16];
    let mut nonce = [0; 12];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|_| Error::Unavailable)?;
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| Error::Unavailable)?;
    seal_with(key, organization, code, salt, nonce)
}
pub(super) fn seal_with(
    key: &SigningKey,
    organization: &str,
    code: &Code,
    salt: [u8; 16],
    nonce: [u8; 12],
) -> Result<Vec<u8>, Error> {
    let ciphertext_len = key.local_der().len() + 16;
    let mut package = Vec::with_capacity(108 + organization.len() + ciphertext_len);
    package.extend_from_slice(MAGIC);
    package.extend_from_slice(&(organization.len() as u16).to_be_bytes());
    package.extend_from_slice(organization.as_bytes());
    package.push(1);
    package.extend_from_slice(&salt);
    package.extend_from_slice(&nonce);
    package.extend_from_slice(&key.public_key());
    package.extend_from_slice(&(ciphertext_len as u32).to_be_bytes());
    let derived = derive(code, &salt)?;
    let cipher = LessSafeKey::new(
        UnboundKey::new(&aead::AES_256_GCM, &derived[..]).map_err(|_| Error::Unavailable)?,
    );
    let mut ciphertext = Zeroizing::new(Vec::with_capacity(ciphertext_len));
    ciphertext.extend_from_slice(key.local_der());
    cipher
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(&package)),
            &mut *ciphertext,
        )
        .map_err(|_| Error::Unavailable)?;
    package.extend_from_slice(&ciphertext);
    Envelope::parse(&package, organization)?;
    Ok(package)
}
#[cfg(test)]
thread_local! {pub(super) static KDFS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}
