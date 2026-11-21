//! Public record source is intentionally separate from local CA/key APIs.
use org_office::organization_family::operation_id::from_wire_ulid;
use org_office_launch::recovery as wire;
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufReader},
    net::Shutdown,
    os::unix::net::UnixStream,
    time::Duration,
};

pub(super) fn serve(
    mut stream: UnixStream,
    office: &org_office::Office,
    instance: &str,
    office_id: &str,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = wire::read_request(&mut BufReader::new(&mut stream))?;
    if request.instance_id != instance
        || request.office_id != office_id
        || office.info().office_id != office_id
        || request.organization_id != office.info().org_id
        || !office.is_hq().map_err(|_| wire::invalid())?
    {
        return Err(wire::invalid());
    }
    let public = office
        .hq_creation_public_record()
        .map_err(|_| wire::invalid())?
        .ok_or_else(wire::invalid)?;
    // The records name the store operation the wire's ULID maps to; the wire keeps the
    // ULID it was asked with (M7). The mapping drops the ULID's low 6 random bits, so the
    // request must name the creation's own command ID, and its mapping the records'.
    let mapped = from_wire_ulid(&request.operation_id).map_err(|_| wire::invalid())?;
    if request.operation_id != office.creation_command_id()
        || public.operation_id != mapped.as_str()
        || public.organization_id != request.organization_id
    {
        return Err(wire::invalid());
    }
    let record = wire::PublicRecord {
        version: 1,
        instance_id: instance.into(),
        office_id: office_id.into(),
        organization_id: public.organization_id,
        operation_id: request.operation_id,
        escrow_version: public.version,
        public_key: wire::hex(&public.public_key),
        package_sha256: wire::hex(&Sha256::digest(&public.package)),
        package_hex: wire::hex(&public.package),
    };
    wire::write_record(&mut stream, &record)?;
    stream.shutdown(Shutdown::Write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hq_creation::tests::{code, key, World, INSTANCE, OFFICE, ORG};

    fn ask(office: &org_office::Office, operation: &str) -> io::Result<wire::PublicRecord> {
        let (worker, mut host) = UnixStream::pair()?;
        let request = wire::Request {
            version: 1,
            instance_id: INSTANCE.into(),
            office_id: OFFICE.into(),
            organization_id: ORG.into(),
            operation_id: operation.into(),
        };
        wire::write_request(&mut host, &request)?;
        host.shutdown(Shutdown::Write)?;
        let served = serve(worker, office, INSTANCE, OFFICE);
        let record = wire::read_record(&mut BufReader::new(host));
        served?;
        let record = record?;
        assert_eq!(record.binding(), request);
        Ok(record)
    }

    /// The recovery-public read returns the HQ's escrow package, bound to the request as the
    /// host made it (the wire's ULID), though the records name the mapped store operation.
    #[test]
    fn the_recovery_public_read_returns_the_escrow_bound_to_the_wire_request() {
        let world = World::new();
        let office = world.prepare(Some(key()), code()).unwrap();
        let operation = crate::hq_creation::tests::binding().operation_id;
        let record = ask(&office, &operation).unwrap();
        let package = record.package().unwrap();
        assert_eq!(
            package,
            std::fs::read(world.roots.durable.join("state/security/ca-escrow.v1.bin")).unwrap()
        );
        assert_eq!(record.package_sha256, wire::hex(&Sha256::digest(&package)));
        // Another operation is not this HQ's creation. (The mapping keeps 74 of the ULID's 80
        // random bits, so this one differs above the dropped low 6.)
        assert!(ask(&office, "01ARZ3NDEKTSV4RRFFQ79G5FAW").is_err());
    }

    /// A ULID that differs from the creation's only in the low 6 random bits maps to the
    /// same store operation, yet it is not the request that created this HQ: refused.
    #[test]
    fn a_ulid_differing_only_in_its_dropped_bits_is_refused() {
        let world = World::new();
        let office = world.prepare(Some(key()), code()).unwrap();
        let operation = crate::hq_creation::tests::binding().operation_id;
        assert_eq!(office.creation_command_id(), operation);
        // The last Crockford character carries the low 5 bits; the one before it, the 6th.
        let mut near = operation.clone();
        let last = near.pop().unwrap();
        near.push(if last == '0' { '1' } else { '0' });
        assert_ne!(near, operation);
        assert_eq!(
            from_wire_ulid(&near).unwrap(),
            from_wire_ulid(&operation).unwrap(),
            "the two ULIDs map to one store operation"
        );
        assert!(ask(&office, &near).is_err());
        ask(&office, &operation).unwrap();
    }
}
