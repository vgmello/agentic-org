//! What an office folder records about its organization's data (`TASK-104`): the data roots
//! and the writer reservation its frozen creation (HQ) or join (satellite) intent holds. Read
//! without the office lock and without opening anything else, for a backup or restore that
//! takes the lock itself.

use std::path::Path;

use org_store::{Id, OfficeIdentity};

use super::{intent, satellite, DataRoots};
use crate::{error::OfficeError, model::OfficeInfo, office_file};

/// An office as its folder records it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecordedOffice {
    pub info: OfficeInfo,
    pub roots: DataRoots,
    /// The journal identity the host reserved for it (instance, organization, office,
    /// journal incarnation, writer epoch, machine).
    pub identity: OfficeIdentity,
}

fn id(text: &str) -> Result<Id, OfficeError> {
    Id::parse(text).map_err(|_| OfficeError::Invalid("an office intent holds an invalid id"))
}

/// Reads `folder`'s `office.json` and its intent. `None` when the folder holds no finished
/// office; an intent that does not name the folder's office is refused.
pub fn recorded_office(folder: &Path) -> Result<Option<RecordedOffice>, OfficeError> {
    let Some(info) = office_file::read(folder)? else {
        return Ok(None);
    };
    let found = match info.role {
        crate::model::OfficeRole::Hq => intent::read(folder)?.map(|i| {
            let roots = i.roots();
            let parts = (
                i.instance,
                i.journal_incarnation,
                i.writer_epoch,
                i.machine_id,
            );
            (i.office_id, i.org_id, roots, parts)
        }),
        crate::model::OfficeRole::Satellite => satellite::read_intent(folder)?.map(|i| {
            let roots = i.roots();
            let parts = (
                i.instance,
                i.journal_incarnation,
                i.writer_epoch,
                i.machine_id,
            );
            (i.office_id, i.org_id, roots, parts)
        }),
    };
    let Some((office_id, org_id, roots, (instance, incarnation, epoch, machine))) = found else {
        return Err(OfficeError::Invalid("the office folder holds no intent"));
    };
    if office_id != info.office_id || org_id != info.org_id {
        return Err(OfficeError::Invalid(
            "the office intent names another office",
        ));
    }
    let identity = OfficeIdentity {
        instance_id: id(&instance)?,
        organization_id: id(&org_id)?,
        office_id: id(&office_id)?,
        journal_incarnation: id(&incarnation)?,
        writer_epoch: epoch,
        machine_id: id(&machine)?,
    };
    Ok(Some(RecordedOffice {
        info,
        roots,
        identity,
    }))
}
