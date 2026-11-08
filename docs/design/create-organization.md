# Flow: Create organization (phase 1)

Status: **draft**. ada writes it with the app agent and the server agent, and they refine it before building.

## Goal
On a Mac with nothing set up, the owner opens agentic.org, creates an organization, and lands in the app shell connected to a running office server on this machine.

## The flow
1. **First launch:** two choices, **Create organization** and **Open organization** (open is phase 2).
2. **Create:** the owner enters the organization's name (and later a description). The app shows where its office will run: "This Mac".
3. **Start the office:** the app makes sure `org-office-server` is installed and **registered as a login service**, starts it if needed, and connects through `org-client` (`hello`, a version check).
4. **Create:** the app calls `organization.create { name }`. The server creates the organization's storage (the workspace folder and its SQLite index) and returns the organization.
5. **Done:** the app shows the shell (sidebar, an empty agents list, "Hire your first agent") and remembers the connection, so the next launch opens straight into it. The menu-bar item lists the office.

## API (`org-proto`, first version, to be agreed)
- `hello() → { api_version, server_version, office: { id, name, host: "This Mac" } }`
- `organization.create({ name }) → Organization` (errors: `invalid_name`, `already_exists`, `storage_error`)
- `organization.list() → Organization[]`, so a server can hold several organizations
- an event stream (`subscribe`) for later screens

## States to design
Server not installed · login service not approved by the user (System Settings) · server starting · server unreachable · name invalid · organization already exists · created.

## Open questions
1. Where an organization's data lives on disk (proposal: `~/Library/Application Support/agentic.org/organizations/<id>/`, holding the workspace and index).
2. Whether creating an organization also sets up HR and the role catalogue right away, or later.
3. Can one server host several organizations? (Proposal: yes, and the app's dropdown shows server → organizations.)
