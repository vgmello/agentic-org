# Flow: Create organization (phase 1)

Status: **draft**. ada writes it with the app agent and the server agent, and they refine it before building.

## Goal
On a Mac with nothing set up, the owner opens Agentic Org, creates an organization, and lands in the app shell connected to the organization's HQ office on this machine.

## The flow
1. **First launch:** two choices, **Create organization** and **Open organization**. Open has two paths: open an organization this Mac already has (it lists them), or join an organization from another machine with an invitation link (offices design, phase 3).
2. **Create:** the owner enters the organization's name (and later a description). The app shows where its office will run: "This Mac".
3. **Start the host:** the app makes sure the host (`org-office-host`, bundled inside it) is **registered as a login service**, starts it if needed, and connects through `org-client` (`hello`, a version check). The host starts each office's server.
4. **Create:** the app calls `organization.create { name }`. The host starts a new office server with `--create`, and that server creates the HQ office: its own key, its databases and `office.json`. Once the server reports the office created, the host returns the organization with that office. A create that fails is cleaned up and returned to the app as an error.
5. **Done:** the app shows the shell (sidebar, an empty agents list, "Hire your first agent") and remembers the organization, so the next launch opens straight into it. The menu-bar dropdown lists organizations first, each with its offices under it.

## API (`org-proto` v1, revised 2026-11-08 for the office model in `offices-hq-and-satellites.md`; the crate's docs are the reference)
The app talks to this Mac's **host** (`org-office-host`) for these calls. Each office then has its own server.
- `hello() → { api_version, server_version, host: "This Mac" }`. An office server's `hello` also carries `office: { organization_name, office }`.
- `organization.create({ name }) → Organization { id, name, created_at, offices: [its HQ office] }`. It starts a `--create` server on this Mac, which creates the organization's HQ office. It succeeds once the server reports the office created (its key, databases and `office.json` durable); the HQ office's `status` then says whether it's serving (`starting`, `active`, `unavailable`). A create that fails is cleaned up and returned as an error. (errors: `invalid_name`, `already_exists`, `storage_error`). The name is trimmed, 1 to 64 characters, with no control characters, and unique (ignoring case) among this Mac's organizations. The host checks the name and its uniqueness, and returns `already_exists` for a clash. It runs creates one at a time and counts the names of creates still in flight, so two creates with the same name can't both pass. The `--create` server validates the name again. If the server crashes after `office.json` is durable but before it reports "created", the office exists: the host treats `office.json` as the marker, so `organization.create` succeeds, and the HQ office's `status` shows `starting` until the restarted server is ready.
- `organization.list() → { organizations: Organization[] }`: every organization with an office on this Mac, each with its offices here (id, role, host, endpoint, status).
- `office.list() → { offices: OfficeInfo[] }`: every office on this Mac and its socket.
- `office.app_token({ office_id }) → { app_token }`: the office's `app_token` (ADR-0003 §2.1), 32 random bytes as 64 lowercase hex. On `host.sock` only, and the caller must pass the peer code-signature check. (errors: `not_authorized` if the check failed, `not_found` for an unknown office, `unavailable` if the office isn't running)
- `subscribe() → {}`, after which the host pushes events, starting with `organization.created`
- Any request can also fail with `invalid_request` or `unknown_method`. Organization and office ids are opaque strings (ULIDs); request ids are numbers. `created_at` is RFC 3339 UTC.

## States to design
Login service not registered · login service not approved by the user (System Settings) · host or HQ office starting · host unreachable · HQ office failed to start · name invalid · organization already exists · created.

## Open questions
1. Where an organization's data lives on disk (proposal: `~/Library/Application Support/agentic.org/organizations/<id>/`, holding the workspace and index).
2. Whether creating an organization also sets up HR and the role catalogue right away, or later.
3. Can one server host several organizations? (Proposal: yes, and the app's dropdown shows server → organizations. The API already allows it via `organization.list`; waiting on the user.)
