//! A CA fixture: material whose escrow package is framed as ADR-0007 says, for
//! `organization`, with fixed bytes. Its ciphertext opens with no code: tests of the store
//! never open it.

use org_store::Id;

use super::CaMaterial;

/// The fixture's public key: uncompressed SEC1, `seed` everywhere after the `0x04`.
pub fn public_key(seed: u8) -> [u8; 65] {
    let mut key = [seed; 65];
    key[0] = 4;
    key
}

/// An escrow package of `organization` with `public_key`: framing only.
pub fn package(organization: &Id, public_key: &[u8; 65], seed: u8) -> Vec<u8> {
    let organization = organization.as_str().as_bytes();
    let mut package = b"AGORGCA1".to_vec();
    let length = u16::try_from(organization.len()).expect("an organization ID is short");
    package.extend(length.to_be_bytes());
    package.extend(organization);
    package.push(1);
    package.extend([seed; 16 + 12]);
    package.extend(public_key);
    let ciphertext = [seed.wrapping_add(1); 48];
    package.extend(u32::try_from(ciphertext.len()).unwrap().to_be_bytes());
    package.extend(ciphertext);
    package
}

/// Material of `organization` made from `seed`: another seed is material generated again.
pub fn ca_material(organization: &Id, seed: u8) -> CaMaterial {
    let public_key = public_key(seed);
    CaMaterial {
        package: package(organization, &public_key, seed),
        public_key,
        wrapped_ca_key: vec![seed ^ 0x5a; 64],
    }
}
