# Flow: Create organization (phase 1)

Status: **draft**. ada writes it with the app agent and the server agent, and they refine it before building.

## Goal
On a Mac with nothing set up, the owner opens agentic.org, creates an organization, and lands in the app shell connected to a running office server on this machine.

## The flow
1. **First launch:** two choices, **Create organization** and **Open organization** (open is phase 2).
2. **Create:** the owner enters the organization's name (and later a description). The app shows where its office will run: "This Mac".
3. **Start the office:** the app makes sure the `org-office-server` bundled inside it is **registered as a login service**, starts it if needed, and connects through `org-client` (`hello`, a version check).
4. **Create:** the app calls `organization.create { name }`. The server creates the organization's storage (its workspace folder; a SQLite index comes when something indexes) and returns the organization.
5. **Done:** the app shows the shell (sidebar, an empty agents list, "Hire your first agent") and remembers the connection, so the next launch opens straight into it. The menu-bar item lists the office.

## API (`org-proto` v1, agreed by ada with the app and server agents 2026-11-08; the crate's docs are the reference)
- `hello() → { api_version, server_version, office: { id, name, host: "This Mac" } }`
- `organization.create({ name }) → Organization { id, name, created_at }` (errors: `invalid_name`, `already_exists`, `storage_error`). The name is trimmed, 1 to 64 characters, with no control characters, and unique ignoring case. The server enforces the rules.
- `organization.list() → { organizations: Organization[] }`: every organization the server holds (the shape leaves room for several; see open question 3)
- `subscribe() → {}`, after which the server pushes events, starting with `organization.created`
- Any request can also fail with `invalid_request` or `unknown_method`. Organization and office ids are opaque strings (ULIDs); request ids are numbers. `created_at` is RFC 3339 UTC.

## States to design
Login service not registered · login service not approved by the user (System Settings) · server starting · server unreachable · name invalid · organization already exists · created.

## Open questions
1. Where an organization's data lives on disk (proposal: `~/Library/Application Support/agentic.org/organizations/<id>/`, holding the workspace and index).
2. Whether creating an organization also sets up HR and the role catalogue right away, or later.
3. Can one server host several organizations? (Proposal: yes, and the app's dropdown shows server → organizations. The API already allows it via `organization.list`; waiting on the user.)
