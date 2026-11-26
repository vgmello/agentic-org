//! Public record source is intentionally separate from local CA/key APIs.
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
    if !org_proto::is_uuid_v7(&request.operation_id)
        || request.operation_id != office.creation_command_id()
        || public.operation_id != request.operation_id
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
mod tests;
