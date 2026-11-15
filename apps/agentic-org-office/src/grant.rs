//! Worker-owned constant-time grant verification; protocol types stay crypto-free.
use hmac::{Hmac, Mac};
use org_proto::{grant_mac_input, Grant, GrantKey, GrantMac};
use sha2::Sha256;

pub(crate) fn verify(key: &GrantKey, grant: &Grant) -> bool {
    verify_input(key, grant_mac_input(&grant.claims).as_deref(), &grant.mac)
}

fn verify_input(key: &GrantKey, input: Option<&[u8]>, tag: &GrantMac) -> bool {
    let Some(input) = input else { return false };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key.expose_bytes()) else {
        return false;
    };
    mac.update(input);
    // No tag array is copied out. HMAC's internal-state zeroing is an X-04 residual.
    mac.verify_slice(tag.expose_bytes()).is_ok()
}

#[cfg(test)]
mod tests;
