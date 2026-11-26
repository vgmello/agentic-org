//! Organization office membership registration (HQ-only). Applying the `state/` files a
//! satellite replicates from HQ is `crate::replication::state_files`.

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Operation, Refusal, UnitOfWork};

use super::{invalid, operation, reply, shared, Feature, Op};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, Meta, Office, OfficeCode, OfficeRole},
};

/// Registers (or idempotently refreshes) an enrolled satellite office in `state/offices/`
/// and allocates a unique office code in `state/identity/office-codes.yaml`.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct RegisterSatelliteOffice {
    pub office_id: Id,
    pub office_name: String,
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct SatelliteRegistered {
    pub office_id: Id,
    pub slug: String,
    pub office_code: String,
    pub generation: u64,
}

impl Feature for RegisterSatelliteOffice {
    const OP: Op = Op::RegisterSatelliteOffice;

    fn operation(&self, id: Id, actor: &Actor) -> Operation {
        operation(Self::OP, id, actor.clone(), self)
    }

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        let admitted = shared::admit(uow, here, owner, by)?;
        if !records::is_stable_id(self.office_id.as_str()) {
            return invalid("satellite office_id must be a canonical lowercase UUIDv7");
        }
        let name = self.office_name.trim();
        if !records::is_line(name) {
            return invalid("satellite office_name is invalid");
        }
        let offices = uow.view().offices()?;
        let mut codes = uow.view().office_codes()?;

        // Idempotent refresh when this satellite is already registered (e.g. D25 re-enrollment).
        if let Some(existing) = offices.iter().find(|o| o.id == self.office_id) {
            if existing.role != OfficeRole::Satellite {
                return invalid("cannot register HQ office as satellite");
            }
            let code = codes
                .iter()
                .find(|c| c.office_id == self.office_id && c.retired_at.is_none())
                .map(|c| c.code.clone())
                .unwrap_or_else(|| next_office_code(&codes));
            let updated = Office {
                name: name.to_owned(),
                meta: shared::touched(&existing.meta, &admitted.by, admitted.now),
                ..existing.clone()
            };
            let result = SatelliteRegistered {
                office_id: updated.id.clone(),
                slug: updated.slug.clone(),
                office_code: code,
                generation: admitted.generation,
            };
            return Ok(Change::new(Self::OP.ack())
                .put(
                    records::office_path(&updated.id),
                    records::encode_office(&updated),
                )
                .reply(reply(&result)));
        }

        let slug = allocate_office_slug(name, &offices);
        let code_str = next_office_code(&codes);
        let meta = Meta::created(&admitted.by, admitted.now);
        let office = Office {
            id: self.office_id.clone(),
            slug: slug.clone(),
            name: name.to_owned(),
            role: OfficeRole::Satellite,
            meta,
        };
        codes.push(OfficeCode {
            code: code_str.clone(),
            office_id: self.office_id.clone(),
            issued_at: records::time_text(admitted.now),
            retired_at: None,
        });
        let result = SatelliteRegistered {
            office_id: office.id.clone(),
            slug,
            office_code: code_str,
            generation: admitted.generation,
        };
        Ok(Change::new(Self::OP.ack())
            .put(
                records::office_path(&office.id),
                records::encode_office(&office),
            )
            .put(
                records::office_codes_path(),
                records::encode_office_codes(&codes),
            )
            .reply(reply(&result)))
    }
}

fn allocate_office_slug(name: &str, existing: &[Office]) -> String {
    let mut base = String::new();
    let mut prev_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            base.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            base.push('-');
            prev_dash = true;
        }
    }
    while base.ends_with('-') {
        base.pop();
    }
    if base.is_empty() {
        base.push_str("office");
    }
    base.truncate(56);
    while base.ends_with('-') {
        base.pop();
    }
    if !existing.iter().any(|o| o.slug == base) {
        return base;
    }
    for suffix in 2u32..10_000 {
        let candidate = format!("{base}-{suffix}");
        if !existing.iter().any(|o| o.slug == candidate) {
            return candidate;
        }
    }
    format!("office-{}", existing.len() + 1)
}

fn next_office_code(existing: &[OfficeCode]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ"; // 24 letters: excludes I and O
    for &b in ALPHABET {
        let candidate = (b as char).to_string();
        if !existing.iter().any(|c| c.code == candidate) {
            return candidate;
        }
    }
    for &a in ALPHABET {
        for &b in ALPHABET {
            let candidate = format!("{}{}", a as char, b as char);
            if !existing.iter().any(|c| c.code == candidate) {
                return candidate;
            }
        }
    }
    "ZZ".to_owned()
}
