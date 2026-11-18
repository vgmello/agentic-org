//! Deterministic identifiers for tests. The store mints none.

use crate::{change::Id, custody::OfficeIdentity};

/// The `n`th test ID, in UUIDv7 text form.
pub fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).expect("a valid test ID")
}

/// One office of one organization, writer epoch 1.
pub fn identity() -> OfficeIdentity {
    OfficeIdentity {
        instance_id: id(0xA1),
        organization_id: id(0xA2),
        office_id: id(0xA3),
        journal_incarnation: id(0xA4),
        writer_epoch: 1,
        machine_id: id(0xA5),
    }
}
