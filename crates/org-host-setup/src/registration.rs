//! Steps 3 and 5: reserving the instance and registering and starting its service (SRV-6).

use std::ffi::OsString;
use std::io::Write;
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_host_control::admin::{AdminLinkInitial, AdminLinkInitialParams, AdminLinkResult};
use org_host_control::Administrator;
use org_platform::service::{ServiceStatus, Unit};

use org_host::instance::registry::Registry;
use org_host::instance::setup::{Input, Journal, Record, Service};

use super::account::user_name;
use super::adapters::{CredentialDirectory, Platform};
use super::address::{address_free, port_held_elsewhere};
use super::credentials::{
    make_credential_directory, open_as_the_host_will, unused_as_the_host_will,
    unused_mounted_store, use_credential_directory,
};
use super::output::{fail, say};
use super::report::{lingering_report, running_with_administrator, service_id, show_link};
use super::request::Request;
use super::wire::{connect, ready};

const READY_WITHIN: Duration = Duration::from_secs(60);

pub(super) fn reserve_setup_instance(
    registry: &Registry,
    journal: &Journal,
    platform: &Platform,
    records: BorrowedFd<'_>,
    name: Option<&str>,
    record: &mut Record,
    froze_from_file: bool,
) -> Result<(String, PathBuf), String> {
    match &record.instance_id {
        Some(recorded) => {
            // The runtime folders are not kept: a cache that was cleared since (a container
            // started on its volumes again) has none, and they are made again here, as the
            // host makes them at its start (X5 review, fourth round, P1-4).
            registry
                .restore_temp_folders()
                .map_err(|e| fail("restoring instance temp folders", e))?;
            let (id, temp) = registry
                .locate(name, &record.input.root)
                .map_err(|e| fail("locating the instance", e))?;
            if &id != recorded {
                return Err(
                    "the reserved instance is not the one setup recorded; nothing was changed"
                        .into(),
                );
            }
            Ok((id, temp.join("host.sock")))
        }
        None => {
            let reserve = |record: &mut Record| -> Result<(String, PathBuf), String> {
                registry
                    .restore_temp_folders()
                    .map_err(|e| fail("restoring instance temp folders", e))?;
                let reserved = {
                    let instance = registry
                        .reserve(name, &record.input.root)
                        .map_err(|e| fail("reserving the instance", e))?;
                    instance
                        .set_browser(&record.input.listen)
                        .map_err(|e| fail("recording the instance address", e))?;
                    instance
                        .prepare_layout()
                        .map_err(|e| fail("preparing the instance folder", e))?;
                    // While the root lock is held: the directory is checked against the root
                    // itself, and recorded together with the instance it belongs to.
                    record.credentials = match (&record.input.credentials, &platform.credentials) {
                        // The configuration names it. In the foreground mode it is a mounted
                        // volume, which setup never makes.
                        (Some(configured), _) if record.input.service == Service::Foreground => {
                            Some(unused_mounted_store(
                                configured,
                                instance.root_descriptor(),
                                records,
                            )?)
                        }
                        (Some(configured), _) => Some(use_credential_directory(
                            configured,
                            instance.root_descriptor(),
                            Path::new("/"),
                            unused_as_the_host_will(records),
                        )?),
                        (None, CredentialDirectory::Below(base)) => {
                            Some(make_credential_directory(
                                base,
                                instance.id(),
                                instance.root_descriptor(),
                                Path::new("/"),
                                open_as_the_host_will(records),
                            )?)
                        }
                        (None, CredentialDirectory::Unused) => None,
                    };
                    (
                        instance.id().to_owned(),
                        instance.temp_folder().join("host.sock"),
                    )
                };
                record.instance_id = Some(reserved.0.clone());
                journal.write(record).map_err(|e| fail("saving setup", e))?;
                Ok(reserved)
            };
            reserve(record).map_err(|refused| {
                refused_reservation(
                    refused,
                    registry,
                    journal,
                    froze_from_file,
                    name,
                    &record.input,
                )
            })
        }
    }
}

pub(super) fn start_registered_setup(
    platform: &Platform,
    request: Request<'_>,
    out: &mut dyn Write,
    journal: &Journal,
    record: &mut Record,
    instance_id: &str,
    socket: &Path,
) -> Result<(), String> {
    let name = request.name.as_deref();
    let shown = name.unwrap_or("default");
    let service_id = service_id(instance_id);
    let unit = Unit {
        instance_id: service_id.clone(),
        program: request.program,
        args: ["serve", "--name", shown].map(OsString::from).to_vec(),
        env: request.env,
    };
    platform
        .service
        .register(&unit)
        .map_err(|e| fail("registering the service", e))?;
    if !record.service_registered {
        record.service_registered = true;
        journal.write(record).map_err(|e| fail("saving setup", e))?;
    }
    say(out, "Service registered.")?;
    for line in lingering_report(&platform.service.lingering(), &user_name()) {
        say(out, line)?;
    }
    let listen = &record.input.listen;
    match platform.service.status(&service_id) {
        Ok(ServiceStatus::Running { .. }) => {}
        _ => {
            // Nothing of this instance runs, so a held address belongs to another program.
            if !address_free(listen) {
                return Err(port_held_elsewhere(listen, name));
            }
            platform
                .service
                .start(&service_id)
                .map_err(|e| fail("starting the service", e))?
        }
    }

    // 6. Readiness: hello and instance.status for this instance, with its browser service
    // running, within the deadline. Until then nothing says the browser can be used.
    let status = ready(socket, instance_id, READY_WITHIN, || {
        // A failed browser service no longer holds its address; if something else does,
        // that is the reason.
        (!address_free(listen)).then(|| port_held_elsewhere(listen, name))
    })?;
    let administrator = status.administrator;
    say(out, "Service running.")?;
    if !record.registry_created {
        record.registry_created = true;
        journal.write(record).map_err(|e| fail("saving setup", e))?;
    }

    // 7. The initial link, at most once.
    if administrator == Administrator::Present {
        return say(
            out,
            running_with_administrator(shown, record.input.listen.origin().as_str()),
        );
    }
    say(out, "Instance ready. It has no administrator yet.")?;
    // The link is shown only in a terminal; noninteractive setup does not ask for it.
    let result = if record.initial_link_issued || !request.terminal {
        AdminLinkResult::AlreadyIssued
    } else {
        let mut wire = connect(socket, instance_id)?;
        let result = wire
            .call::<AdminLinkInitial>(
                &AdminLinkInitialParams {
                    setup_operation: record.setup_operation.clone(),
                },
                None,
            )
            .map_err(|e| fail("asking for the registration link", e))?;
        record.initial_link_issued = true;
        journal.write(record).map_err(|e| fail("saving setup", e))?;
        result
    };
    show_link(out, result, name, record.input.listen.origin())
}

/// What a reservation that refused says: its reason, then what the registry holds for the
/// instance now, read from the locator and not from how far the reservation got (it can be
/// saved before a later step of it fails), and an error of the record's cleanup when there
/// was one (X5 review, fourth round, P2-7).
///
/// What a run froze from a file is taken away, so that it does not refuse the next,
/// corrected file. A reservation that was made stays the registry's: the same data folder
/// and address are reserved again by the next run. Both were asked before the reservation
/// (`preflight`), so what remains reserved can be served; what refused after it (the
/// credential directory, or the disk) is not part of it, and a file that corrects only that
/// continues the setup. Known limit: the registry has no way to undo a reservation, so a
/// file with another data folder or address is refused by it until the instance is removed.
pub(super) fn refused_reservation(
    refused: String,
    registry: &Registry,
    journal: &Journal,
    froze_from_file: bool,
    name: Option<&str>,
    input: &Input,
) -> String {
    let shown = name.unwrap_or("default");
    let cleanup = froze_from_file.then(|| journal.abandon());
    let mut said = refused;
    match registry.reserved(name) {
        Ok(false) => {}
        Ok(true) if froze_from_file => said.push_str(&format!(
            "; instance \"{shown}\" stays reserved with the data folder {} and the address {}: \
             run setup again with a file that keeps both (it may change the credential \
             directory); another data folder or address needs the instance removed, which no \
             command offers yet",
            input.root.display(),
            input.listen.origin().as_str()
        )),
        Ok(true) => said.push_str(&format!(
            "; instance \"{shown}\" stays reserved with the data folder {} and the address {}: \
             run setup again to continue",
            input.root.display(),
            input.listen.origin().as_str()
        )),
        Err(e) => said.push_str(&format!(
            "; whether instance \"{shown}\" stays reserved could not be read ({e}): run setup \
             again with the same input to continue"
        )),
    }
    if let Some(Err(e)) = cleanup {
        said.push_str(&format!(
            "; the unfinished setup record could not be removed ({e}), so only the same file \
             continues this setup"
        ));
    }
    said
}
