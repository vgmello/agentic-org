# Design: create an organization through standalone HQ

Status: **proposed implementation**, 2026-11-09, following the owner's v1 pivot.
Owner: ada. Replaces the undeployed Tauri/SMAppService creation flow; there is no
old-app import or compatibility path. See [ADR-0005](../adr/0005-standalone-server-v1.md)
and [server setup requirements](../specs/server-installation-and-access-ux.md).

## Setup and ownership

Standalone setup selects `default` or an explicitly named instance. A new HQ setup
establishes owner credentials before exposing administration. It can create the
first organization using the same domain operation as the authenticated HTTP API.
A satellite setup instead enrolls an office in an existing organization. Opening
HQ in a browser performs neither installation nor enrollment.

## Create flow

1. An authorized owner chooses **Create organization** and enters the name. The
   browser shows which HQ instance will store it. No local path picker appears.
2. The request carries an operation ID and the proposed organization details.
   Server authorization and validation run before provisioning.
3. The instance reserves the normalized name and IDs under its catalog lock, then
   performs slow provisioning outside that lock. Creation is idempotent by
   operation ID; concurrent requests cannot create duplicate names or identities.
4. The host creates the office key and starts its HQ worker through the private
   launch contract. The worker owns its folder, databases and durable `office.json`
   marker. Preserve the existing adopt-versus-discard rules after a crash.
5. Return a durable result containing organization/office IDs and readiness. The
   browser opens the organization, showing Starting until its office is ready.
   If the response is lost, retry or look up the same operation instead of creating
   a second organization. A saved organization may be stopped/unavailable; that is
   distinct from a failed create.

The server provisions agent homes and the built-in HR flow through their existing
domain contracts. The new organization begins without imported HR/spool records.
Owner/team/role/hiring behavior remains in the corresponding specs.

## Reads and failures

HQ lists only organizations the browser principal may access. Last-selected
organization is a UI preference; it contains no privileged office token. Unknown
or unauthorized organization IDs do not expose metadata. Invalid name, duplicate
name, unavailable key store, failed worker, storage error and temporary starting
state have distinct actionable responses. Status/event reconnect follows the
standalone server's snapshot/cursor contract. Service start/repair belongs to the
local CLI and works even if the HTTP server is stopped.

## Verification

Test repeated/lost create responses, competing name reservations, interruption
before and after the durable office marker, unavailable credentials, unauthorized
browser access and create success followed by worker failure. Use disposable roots;
never import or reset a user's existing organization as a test fixture.
