//! The built-in Chief of Staff (Tess, `TASK-80.1`, `doc-97`): who she is on record, and the one
//! place her record is built, for the organization's creation and for the idempotent bootstrap
//! of an organization created before she existed.
//!
//! Her display name is `Tess` and her title is `Chief of Staff`. The expansion of her name
//! lives in her profile's body (her charter) and nowhere else: never in her name, handle or
//! title.

use org_store::{Id, Sha256};

use super::records::{self, BuiltinKey, Employee, EmployeeKind, EmployeeStatus, Meta};

/// Her display name.
pub const CHIEF_OF_STAFF_NAME: &str = "Tess";
/// Her short title, as her profile and the directory show it.
pub const CHIEF_OF_STAFF_TITLE: &str = "Chief of Staff";
/// Her charter: the body of her profile, and only there.
pub const CHIEF_OF_STAFF_CHARTER: &str = "Tactical Execution & Squad Supervisor";
/// The handles she prefers, in order: the first nobody holds yet. `tess` is her default, beside
/// the HR lead's `hr`; the others keep her provisionable when a person or agent already holds
/// `tess`.
pub const CHIEF_OF_STAFF_HANDLES: [&str; 3] = ["tess", "chief-of-staff", "tess-cos"];
/// The numbered handles she falls back to after [`CHIEF_OF_STAFF_HANDLES`]:
/// `tess-cos-2` through `tess-cos-256`, as many as her ID has derivations, so an organization
/// whose members already hold every preferred handle still gets her.
pub const CHIEF_OF_STAFF_NUMBERED_HANDLES: std::ops::RangeInclusive<u16> = 2..=256;
/// The prefix of a numbered fallback handle.
const NUMBERED_HANDLE_PREFIX: &str = "tess-cos-";

/// The tag her ID is derived under (see [`chief_of_staff_id`]).
const ID_TAG: &str = "chief_of_staff";
/// The tag of the bootstrap operation's ID, so a retry of the bootstrap is the same operation.
const ENSURE_OPERATION_TAG: &str = "chief_of_staff/ensure";

/// Her employee ID in the organization `organization`: a UUIDv7 that keeps the
/// organization ID's timestamp and takes its other bits from a SHA-256 of the organization ID.
/// The same organization always names the same Tess, so a retried creation, a retried
/// bootstrap and a rebuilt projection agree on who she is without minting anything.
/// `attempt` picks another ID in the unlikely case the first is already some record's.
pub fn chief_of_staff_id(organization: &Id, attempt: u8) -> Id {
    derived_id(organization, ID_TAG, attempt)
}

/// The operation ID of the bootstrap that provisions her in `organization`.
pub fn ensure_operation_id(organization: &Id) -> Id {
    derived_id(organization, ENSURE_OPERATION_TAG, 0)
}

/// The first handle that `in_use` says nobody holds: one of [`CHIEF_OF_STAFF_HANDLES`], else
/// the first free numbered fallback (`tess-cos-2` .. `tess-cos-256`), tried in a fixed order so
/// a retry picks the same one. `None` only when all of them are taken.
pub fn free_handle<E>(
    mut in_use: impl FnMut(&str) -> Result<bool, E>,
) -> Result<Option<String>, E> {
    for handle in CHIEF_OF_STAFF_HANDLES {
        if !in_use(handle)? {
            return Ok(Some(handle.to_owned()));
        }
    }
    for number in CHIEF_OF_STAFF_NUMBERED_HANDLES {
        let handle = format!("{NUMBERED_HANDLE_PREFIX}{number}");
        if !in_use(&handle)? {
            return Ok(Some(handle));
        }
    }
    Ok(None)
}

/// Her record at its first revision: an active agent of `team` reporting to the owner, with
/// hiring power on (the owner turns it off and on), and no hire request behind her. Her role
/// reads `Chief of Staff` (so `org.snapshot` shows it); her charter stays in the body alone.
///
/// Reporting to the owner is written as reporting to no one (`reports_to_employee_id: null`),
/// the form the wire and the standing prompt already read as "reports to the owner": she
/// follows an ownership handover, and never keeps a former owner from being deactivated.
pub fn chief_of_staff_record(id: Id, handle: &str, team: &Id, meta: Meta) -> Employee {
    Employee {
        id,
        handle: handle.to_owned(),
        kind: EmployeeKind::Agent,
        name: CHIEF_OF_STAFF_NAME.to_owned(),
        home_team_id: team.clone(),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: Some(CHIEF_OF_STAFF_TITLE.to_owned()),
        focus: None,
        role_text: Some(CHIEF_OF_STAFF_TITLE.to_owned()),
        title_in_team: Some(CHIEF_OF_STAFF_TITLE.to_owned()),
        offboarded_at: None,
        may_hire: true,
        builtin_key: Some(BuiltinKey::ChiefOfStaff),
        meta,
        body: format!("{CHIEF_OF_STAFF_CHARTER}\n"),
    }
}

/// A UUIDv7 derived from `seed` under `tag`: `seed`'s 48-bit timestamp, the version and
/// variant bits, and the rest from `SHA-256(tag, attempt, seed)`. A seed that is not a stable
/// UUIDv7 (an `Id` admits more) lends no timestamp: the derived ID then starts at the epoch.
fn derived_id(seed: &Id, tag: &str, attempt: u8) -> Id {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    const EPOCH_PREFIX: &str = "00000000-0000";
    let digest = Sha256::of_parts(&[
        b"agentic-org/derived-id\0",
        tag.as_bytes(),
        b"\0",
        &[attempt],
        seed.as_str().as_bytes(),
    ]);
    let mut nibbles = digest
        .as_bytes()
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f]);
    let mut next = || char::from(HEX[usize::from(nibbles.next().unwrap_or(0))]);
    let seed = seed.as_str();
    let mut text = String::with_capacity(36);
    // Only a stable UUIDv7's first 13 bytes are its ASCII timestamp (`xxxxxxxx-xxxx`).
    let timestamp = if records::is_stable_id(seed) {
        seed.get(..13).unwrap_or(EPOCH_PREFIX)
    } else {
        EPOCH_PREFIX
    };
    text.push_str(timestamp);
    text.push_str("-7");
    (0..3).for_each(|_| text.push(next()));
    text.push('-');
    let variant = variant_nibble(next());
    text.push(variant);
    (0..3).for_each(|_| text.push(next()));
    text.push('-');
    (0..12).for_each(|_| text.push(next()));
    Id::parse(&text).expect("a derived ID is a UUIDv7 in lowercase text")
}

/// One of the four RFC 9562 variant nibbles, chosen by a hex digit.
fn variant_nibble(digit: char) -> char {
    let value = digit.to_digit(16).unwrap_or(0);
    char::from(b"89ab"[(value & 3) as usize])
}

#[cfg(test)]
mod tests;
