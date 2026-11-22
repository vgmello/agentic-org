//! The office-local family's projector. It writes no rows: the CA key record is read from
//! its file at opening and by the recovery read, never on a hot path, so its projection is
//! its source hash alone. It owns every path of its family so that a rebuild, which walks
//! `local/offices/<office-id>/`, finds an owner for each.

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::records::{self, Kind};

/// The projector of one office's local files.
pub struct OfficeLocalFiles {
    organization_id: Id,
    office_id: Id,
}

impl OfficeLocalFiles {
    pub fn new(organization_id: Id, office_id: Id) -> Self {
        Self {
            organization_id,
            office_id,
        }
    }
}

impl<E: Engine> Projector<E> for OfficeLocalFiles {
    fn owns(&self, path: &DurablePath) -> bool {
        records::kind_of(path, &self.office_id).is_some()
    }

    fn apply(
        &self,
        _tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        match (records::kind_of(path, &self.office_id), bytes) {
            (Some(Kind::CaKey), Some(bytes)) => {
                let record = records::decode_ca_key(bytes)
                    .map_err(|e| ProjectionError::new(path, e.to_string()))?;
                let ours = record.organization_id == self.organization_id
                    && record.office_id == self.office_id;
                if !ours {
                    let reason = "the record is another organization's or office's";
                    return Err(ProjectionError::new(path, reason));
                }
                Ok(())
            }
            (Some(Kind::CaKey), None) => Ok(()),
            // The marker is the office's note to its next opening, read before the store
            // opens and removed after: never a source. A rebuild that meets it leaves it
            // out; no entry may write it.
            (Some(Kind::CleanStop), _) => Err(ProjectionError::skippable(
                path,
                "the clean-stop marker is not a source",
            )),
            (None, _) => Err(ProjectionError::new(path, "not an office-local file")),
        }
    }
}
