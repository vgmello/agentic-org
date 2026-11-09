# The office host in the app bundle

The app ships the office host and the office server inside its bundle. The host runs as a macOS **login service**
(a LaunchAgent registered with `SMAppService`), so it starts with the Mac and keeps running when the app is closed.

```
Agentic Org.app/Contents/
  MacOS/org-office-host                                  the host (started by launchd)
  MacOS/org-office-server                                the office server (the host starts one per office)
  Library/LaunchAgents/org.agentic.office-host.plist     the login service (label org.agentic.office-host)
```

The plist is `src-tauri/launch-agents/org.agentic.office-host.plist` (its `BundleProgram` is relative to the bundle,
so moving the app keeps working). `tauri.conf.json` (`bundle.macOS.files`) places all three; a test keeps the plist,
the config and the bundle identifier in agreement.

## How the app uses it

The real office is the app's default (`AGENTIC_ORG_OFFICE=mock` picks the invented one for development).
`RealOffice::ensure_running` says `hello` to the host. If nothing is listening it asks
macOS for the login service's state (`src-tauri/src/login_item.rs`):

| macOS says          | The app does                                             | The owner sees                                  |
| ------------------- | -------------------------------------------------------- | ----------------------------------------------- |
| `not_registered`    | registers the service, then looks again                  | (nothing, or one of the rows below)             |
| `enabled`           | waits up to 15 s for the host to answer                  | "Start the office on This Mac", then it works   |
| `requires_approval` | stops                                                    | "Approval needed" + **Open Login Items** button |
| `not_found`         | stops                                                    | "The office isn't installed" (reinstall the app) |

`ping` (the Offices screen's status check) never registers or starts anything. Commands for Settings:
`login_item_status`, `login_item_set(enabled)`, `login_item_open_settings`.

## Building a development copy

```
bun run install:dev      # stage host + server, tauri build, ad-hoc sign, copy to ~/Applications
```

`scripts/stage-office.sh` builds `org-office-host` and `org-office-server` (release) into `src-tauri/office-bin/`
(gitignored). It stops with a message if the host is not in this workspace (`apps/org-office-host` on v1: update your branch).

**A bundle built this way does not start its host until it is signed properly.** The release host keeps the office's keys in the
Mac's keychain and refuses to start without the keychain access group and entitlement of a signed build (key custody, ADR-0002).
Until a Developer ID build exists, development runs the host by hand: follow `docs/runbooks/local-hq.md` (coming with the
host's key custody), and use the app with `AGENTIC_ORG_OFFICE=mock` when no host is running. The helper binaries must be signed
with these identifiers when bundling: `org-office-server` = `com.ellosoft.agentic-org.org-office-server`, `org-office-host` =
`com.ellosoft.agentic-org.org-office-host` (the release host checks them), with the same team, Developer ID and hardened runtime as the app.

With ad-hoc signing there is no stable identity: macOS asks for approval again after each rebuild, and Keychain items the
host stores belong to that build. **With a Developer ID** (ADR-0002, code signing): sign `org-office-host` and
`org-office-server` with the same identity and the hardened runtime inside `stage-office.sh` (the copies keep their
signature when `bundle.macOS.files` places them; they are not signed by `tauri build`), let `bundle.macOS.signingIdentity`
sign the app, notarize, and drop the ad-hoc step in
`install-dev.sh`. The identity must stay the same across updates, or Keychain access breaks.

## Manual checklist (needs a Mac, a bundle and the host)

1. `bun run install:dev`, open `~/Applications/Agentic Org.app` (only that copy: the one left in `target/release/bundle` shares the service label).
2. Create an organization. First run: macOS shows "Background item added"; the app says **Approval needed** with
   **Open Login Items**. The button opens System Settings → General → Login Items.
3. Allow "Agentic Org". Back in the app, **Try again**: the steps finish and the shell opens.
4. Quit the app. `launchctl list | grep org.agentic.office-host` still shows the host; reopen: no approval, no wait.
5. Turn the item off in Login Items, open the app, create an organization: **Approval needed** again.
6. Kill the host (`pkill org-office-host`): launchd restarts it within ~10 s; the app's next call recovers.
7. Move the app to another folder: the service still starts (BundleProgram is relative).
8. Delete `Contents/Library/LaunchAgents/*.plist` from a copy: the app says the office isn't installed.

## Open decision

`login_item_set(false)` (Settings, "off") leaves the service not registered, and the next failed call makes
`ensure_running` register it again. Before the Settings screen lands: either register only on first run, or remember an
explicit "off". Turning it off in macOS Login Items is different (it reads as `requires_approval`) and is respected.
