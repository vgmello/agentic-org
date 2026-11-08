//! Agentic Org: the desktop client. It talks to an office server through `OfficeConnection` and
//! never reads an office's storage itself.
//!
//! Until `org-client` and the office-setup layer exist, the office is `mock::MockOffice`, in
//! every build, so the shell can be run and shown end to end. It is wired in one place (`run`).

mod connections;
mod menu_model;
mod mock;
mod office;
mod org_agent_rules;
mod org_command;
mod org_engine;
mod org_mock;
mod tray;

use connections::{local_office, Connection, ConnectionStore};
use office::{OfficeConnection, SetupError};
use org_command::OrgCommand;
use org_proto::{ApiError, CommandAccepted, HelloResult, OfficeStatus, OrgSnapshot, Organization};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

/// What the shell holds for the UI commands.
struct AppState {
    office: Arc<dyn OfficeConnection>,
    store: ConnectionStore,
    /// The host the last successful `ensure_running` reached ("This Mac").
    connected: Mutex<Option<String>>,
    /// Starting the office is one-at-a-time: the window and the menu bar can both ask at launch.
    start: Mutex<()>,
    /// Counts successful starts, so a caller can tell whether someone else already restarted the
    /// office since it saw a failure.
    epoch: AtomicU64,
}

/// An organization's structure as the UI reads it. `sample` is app-local, not part of `org-proto`.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
struct OrgView {
    snapshot: OrgSnapshot,
    /// True while the snapshot is invented (the mock office), not the organization's own.
    sample: bool,
}

/// An error the UI can tell apart: `source` says which layer failed and `code` is its code.
#[derive(Debug, Serialize)]
struct CommandError {
    source: &'static str,
    code: String,
    message: String,
}

impl CommandError {
    fn local(code: &str, message: impl Into<String>) -> Self {
        Self {
            source: "app",
            code: code.to_string(),
            message: message.into(),
        }
    }
}

fn code_of<T: Serialize>(code: T) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

impl From<SetupError> for CommandError {
    fn from(e: SetupError) -> Self {
        Self {
            source: "setup",
            code: code_of(e.code),
            message: e.message,
        }
    }
}

impl From<ApiError> for CommandError {
    fn from(e: ApiError) -> Self {
        Self {
            source: "api",
            code: code_of(e.code),
            message: e.message,
        }
    }
}

impl AppState {
    /// Make sure the office is running (the window's own check). Calls are serialized.
    fn ensure_running(&self) -> Result<HelloResult, CommandError> {
        let _one_at_a_time = self.start.lock().unwrap_or_else(|e| e.into_inner());
        self.start_locked()
    }

    fn start_locked(&self) -> Result<HelloResult, CommandError> {
        let hello = self.office.ensure_running()?;
        // The epoch moves before the connection is published, so a caller that sees the new
        // connection can't still hold the old epoch (which would make it restart the office again).
        self.epoch.fetch_add(1, Ordering::SeqCst);
        *self.connected.lock().unwrap_or_else(|e| e.into_inner()) = Some(hello.host.clone());
        Ok(hello)
    }

    /// Start the office only if it isn't already connected: the menu bar calls this often, and
    /// starting an office (installing, registering, waiting for it) must not repeat or overlap.
    /// `wanted` is checked once the start lock is held: a caller that has been overtaken while
    /// waiting (an older menu refresh) gives up with `superseded` instead of starting anything.
    fn start_if_needed_while(&self, wanted: impl Fn() -> bool) -> Result<(), CommandError> {
        let _one_at_a_time = self.start.lock().unwrap_or_else(|e| e.into_inner());
        if !wanted() {
            return Err(CommandError::local(
                "superseded",
                "A newer request replaced this one.",
            ));
        }
        if self.ensure_connected().is_ok() {
            return Ok(());
        }
        self.start_locked().map(|_| ())
    }

    /// Start the office again after a failure seen at epoch `seen`, unless someone else already
    /// has (the epoch moved on). Forgetting the connection and starting happen under one hold of
    /// the start lock, so callers that use `with_restart` never see "not connected" in between
    /// (they wait for the restart and retry).
    fn restart_since(&self, seen: u64) -> Result<(), CommandError> {
        let _one_at_a_time = self.start.lock().unwrap_or_else(|e| e.into_inner());
        if self.epoch.load(Ordering::SeqCst) != seen {
            return Ok(());
        }
        *self.connected.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.start_locked().map(|_| ())
    }

    /// Run `f` against a running office. If the office itself fails (it died, or the login service
    /// was turned off since it was started), start it again and try once more; if `f` found the
    /// office momentarily not connected because another caller is restarting it, wait for that and
    /// try once more. A failure that is the app's own (nothing found, nothing saved) is not retried.
    fn with_restart<T>(
        &self,
        f: impl Fn(&Self) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        self.with_restart_while(|| true, f)
    }

    fn with_restart_while<T>(
        &self,
        wanted: impl Fn() -> bool,
        f: impl Fn(&Self) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        self.start_if_needed_while(&wanted)?;
        let seen = self.epoch.load(Ordering::SeqCst);
        match f(self) {
            Err(e) if e.source == "app" && e.code == "not_connected" => {
                self.start_if_needed_while(&wanted)?;
                f(self)
            }
            // The office couldn't be reached or understood. An answer it gave (a storage error, a bad
            // request) is not a reason to restart it.
            // PLACEHOLDER: today the mock stands in for an unreachable office with an `unknown` api
            // code. When `org-client` is wired in, map its transport failures (NotRunning, Io,
            // Timeout, Disconnected) to a `setup`-source error here, and drop the `unknown` arm.
            Err(e) if e.source == "setup" || (e.source == "api" && e.code == "unknown") => {
                self.restart_since(seen)?;
                f(self)
            }
            other => other,
        }
    }

    fn ensure_connected(&self) -> Result<(), CommandError> {
        self.connected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|_| ())
            .ok_or_else(|| CommandError::local("not_connected", "The office isn't running yet."))
    }

    /// Create the organization on this machine (the host starts its HQ office) and remember the
    /// connection to that office.
    ///
    /// Known gap: if the host creates the organization but `remember` then fails (or the app
    /// is closed in between), the organization exists but the app doesn't know it. The Open
    /// organization flow lists it, so the owner can attach to it.
    fn create_organization(&self, name: &str) -> Result<Connection, CommandError> {
        self.ensure_connected()?;
        let organization = self.office.create_organization(name)?;
        self.remember(organization)
    }

    /// The connection for an organization: its office on this machine (HQ first), remembered.
    fn remember(&self, organization: Organization) -> Result<Connection, CommandError> {
        let office = local_office(&organization).cloned().ok_or_else(|| {
            CommandError::local("no_office", "That organization has no office on this Mac.")
        })?;
        let connection = Connection {
            office,
            organization,
        };
        self.store
            .remember(connection.clone())
            .map_err(|e| CommandError::local("remember_failed", e.to_string()))?;
        Ok(connection)
    }

    /// The organizations the connected host has, each with its offices on this machine.
    fn organizations(&self) -> Result<Vec<Organization>, CommandError> {
        self.ensure_connected()?;
        Ok(self.office.list_organizations()?)
    }

    /// Open an organization this machine already has, and remember the connection.
    fn open_organization(&self, id: &str) -> Result<Connection, CommandError> {
        self.ensure_connected()?;
        let organization = self
            .office
            .list_organizations()?
            .into_iter()
            .find(|o| o.id == id)
            .ok_or_else(|| {
                CommandError::local("not_found", "That organization isn't on this Mac any more.")
            })?;
        if local_office(&organization).is_some_and(|o| o.status == OfficeStatus::Unavailable) {
            return Err(CommandError::local(
                "office_unavailable",
                "That organization's office isn't available right now.",
            ));
        }
        self.remember(organization)
    }

    /// The structure of an organization this machine hosts, and whether it is invented.
    fn org_snapshot(&self, organization_id: &str) -> Result<OrgView, CommandError> {
        self.ensure_connected()?;
        Ok(OrgView {
            snapshot: self.office.org_snapshot(organization_id)?,
            sample: self.office.serves_sample_data(),
        })
    }

    /// Send an organization command to HQ. A command carries its own `command_id`, so running it
    /// again after the office was restarted gets the same answer and changes nothing twice.
    fn org_command(
        &self,
        organization_id: &str,
        command: &OrgCommand,
    ) -> Result<CommandAccepted, CommandError> {
        self.ensure_connected()?;
        Ok(self.office.org_command(organization_id, command)?)
    }

    fn connections(&self) -> Result<Vec<Connection>, CommandError> {
        self.store
            .list()
            .map_err(|e| CommandError::local("connections_unreadable", e.to_string()))
    }
}

async fn blocking<T, F>(f: F) -> Result<T, CommandError>
where
    F: FnOnce() -> Result<T, CommandError> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CommandError::local("internal", e.to_string()))?
}

#[tauri::command]
async fn office_ensure_running(
    state: State<'_, Arc<AppState>>,
) -> Result<HelloResult, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.ensure_running()).await
}

#[tauri::command]
async fn organization_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    name: String,
) -> Result<Connection, CommandError> {
    let state = Arc::clone(&state);
    let connection = blocking(move || state.create_organization(&name)).await?;
    tray::refresh(&app);
    Ok(connection)
}

#[tauri::command]
async fn organizations_list(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Organization>, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.with_restart(|s| s.organizations())).await
}

#[tauri::command]
async fn organization_open(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Connection, CommandError> {
    let state = Arc::clone(&state);
    let connection = blocking(move || state.with_restart(|s| s.open_organization(&id))).await?;
    tray::refresh(&app);
    Ok(connection)
}

#[tauri::command]
async fn org_snapshot(
    state: State<'_, Arc<AppState>>,
    organization_id: String,
) -> Result<OrgView, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.with_restart(|s| s.org_snapshot(&organization_id))).await
}

#[tauri::command]
async fn org_command(
    state: State<'_, Arc<AppState>>,
    organization_id: String,
    command: OrgCommand,
) -> Result<CommandAccepted, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.with_restart(|s| s.org_command(&organization_id, &command))).await
}

/// Remembered connections, oldest first (the most recent is last).
#[tauri::command]
async fn connections_list(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Connection>, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.connections()).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // The mock's connections live apart from anything a real office will remember, so a build
            // that has the real client never opens a fake organization.
            let data = app.path().app_data_dir()?;
            let dir = data.join("mock");
            app.manage(Arc::new(AppState {
                office: Arc::new(mock::MockOffice::from_env(dir.clone())),
                store: ConnectionStore::new(dir),
                connected: Mutex::new(None),
                start: Mutex::new(()),
                epoch: AtomicU64::new(0),
            }));
            let dock = tray::DockPreference::load(data);
            tray::apply_dock(app.handle(), dock.shown());
            app.manage(dock);
            tray::create(app.handle())?;
            Ok(())
        })
        // A menu-bar app: closing the window hides it, and the app keeps running.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            office_ensure_running,
            organization_create,
            organizations_list,
            organization_open,
            org_snapshot,
            org_command,
            connections_list
        ])
        .build(tauri::generate_context!())
        .expect("error while building Agentic Org")
        .run(|app, event| {
            // Clicking the Dock icon with no window showing brings the window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                tray::show_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::{MockOffice, Scenario};
    use std::time::Duration;

    fn make_state(scenario: Scenario) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "agentic-org-test-state-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState {
            office: Arc::new(MockOffice::new(scenario, Duration::ZERO)),
            store: ConnectionStore::new(dir.clone()),
            connected: Mutex::new(None),
            start: Mutex::new(()),
            epoch: AtomicU64::new(0),
        };
        (state, dir)
    }

    #[test]
    fn an_organizations_structure_needs_a_running_office_and_says_it_is_sample_data() {
        let (state, dir) = state_with(vec![hosted("a", "Alpha")]);
        let err = state.org_snapshot("a").unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("app", "not_connected"));
        state.ensure_running().unwrap();
        let view = state.org_snapshot("a").unwrap();
        assert!(view.sample, "the mock office's data is invented");
        assert_eq!(view.snapshot.members.iter().filter(|m| m.owner).count(), 1);
        let err = state.org_snapshot("nope").unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("api", "not_found"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_command_needs_a_running_office_and_reaches_hq_once() {
        let (state, dir) = state_with(vec![hosted("a", "Alpha")]);
        let command: OrgCommand = serde_json::from_value(serde_json::json!({
            "method": "team.create",
            "params": {"command_id": "C1", "team_id": "ops", "name": "Ops"}
        }))
        .unwrap();
        let err = state.org_command("a", &command).unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("app", "not_connected"));
        state.ensure_running().unwrap();
        let first = state
            .with_restart(|s| s.org_command("a", &command))
            .unwrap();
        let again = state
            .with_restart(|s| s.org_command("a", &command))
            .unwrap();
        assert_eq!(first, again);
        let refused = state
            .org_command(
                "a",
                &serde_json::from_value(serde_json::json!({
                    "method": "team.delete",
                    "params": {"command_id": "C2", "team_id": "nope", "expected_version": 1}
                }))
                .unwrap(),
            )
            .unwrap_err();
        assert_eq!(
            (refused.source, refused.code.as_str()),
            ("api", "not_found")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_snapshot_failure_the_office_answered_is_not_a_reason_to_restart_it() {
        let (state, dir) = state_with(vec![hosted("a", "Alpha")]);
        state.ensure_running().unwrap();
        let starts_before = state.epoch.load(Ordering::SeqCst);
        let err = state.with_restart(|s| s.org_snapshot("nope")).unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("api", "not_found"));
        assert_eq!(state.epoch.load(Ordering::SeqCst), starts_before);
        // Not connected yet: with_restart starts the office, then answers.
        let (fresh, dir2) = state_with(vec![hosted("a", "Alpha")]);
        assert!(fresh.with_restart(|s| s.org_snapshot("a")).is_ok());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn creating_before_the_office_is_running_is_refused() {
        let (state, dir) = make_state(Scenario::Healthy);
        let err = state.create_organization("Acme").unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("app", "not_connected"));
        assert!(state.connections().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_created_organization_is_remembered() {
        let (state, dir) = make_state(Scenario::Healthy);
        state.ensure_running().unwrap();
        let created = state.create_organization("  Acme  ").unwrap();
        assert_eq!(created.organization.name, "Acme");
        assert_eq!(state.connections().unwrap(), vec![created]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_name_their_layer_and_code() {
        let (state, dir) = make_state(Scenario::NeedsApproval);
        let err = state.ensure_running().unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("setup", "needs_approval"));

        let (state, dir2) = make_state(Scenario::Healthy);
        state.ensure_running().unwrap();
        let err = state.create_organization("   ").unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("api", "invalid_name"));
        assert!(state.connections().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    const COMMANDS: [&str; 7] = [
        "office_ensure_running",
        "organization_create",
        "organizations_list",
        "organization_open",
        "org_snapshot",
        "org_command",
        "connections_list",
    ];

    /// Every command the UI invokes is declared in `build.rs`, allowed in the capability file and
    /// implemented here: a name that drifts in one place would only break the packaged app.
    #[test]
    fn command_names_agree_across_ui_build_script_capability_and_code() {
        let quoted = |text: &str| -> Vec<String> {
            text.split(['\'', '"'])
                .skip(1)
                .step_by(2)
                .filter(|w| {
                    w.len() > 3
                        && w.contains('_')
                        && w.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                })
                .map(str::to_string)
                .collect()
        };
        let mut from_ui = quoted(include_str!("../../src/lib/office/tauri.ts"));
        from_ui.extend(quoted(include_str!("../../src/lib/org/tauri.ts")));
        let mut from_build = quoted(include_str!("../build.rs"));
        let mut declared: Vec<String> = COMMANDS.iter().map(|c| c.to_string()).collect();
        from_ui.sort();
        from_build.sort();
        declared.sort();
        assert_eq!(from_ui, declared, "tauri.ts invokes");
        assert_eq!(from_build, declared, "build.rs declares");
        let capability = include_str!("../capabilities/default.json");
        let source = include_str!("lib.rs");
        for name in COMMANDS {
            let permission = format!("allow-{}", name.replace('_', "-"));
            assert!(capability.contains(&permission), "{permission} not granted");
            assert!(
                source.contains(&format!("async fn {name}(")),
                "{name} not implemented"
            );
            assert!(
                source.contains(&format!("            {name},"))
                    || source.contains(&format!("            {name}\n")),
                "{name} not registered"
            );
        }
    }

    fn office(id: &str, role: org_proto::OfficeRole) -> org_proto::OfficeInfo {
        org_proto::OfficeInfo {
            id: id.into(),
            organization_id: "org-1".into(),
            name: id.into(),
            role,
            host: "This Mac".into(),
            endpoint: None,
            status: org_proto::OfficeStatus::Active,
        }
    }

    fn organization_with(offices: Vec<org_proto::OfficeInfo>) -> Organization {
        Organization {
            id: "org-1".into(),
            name: "Acme".into(),
            created_at: "2026-11-08T02:10:00.000Z".into(),
            offices,
        }
    }

    #[test]
    fn the_connection_uses_the_hq_office_even_when_it_is_not_first() {
        let (state, dir) = make_state(Scenario::Healthy);
        let connection = state
            .remember(organization_with(vec![
                office("sat", org_proto::OfficeRole::Satellite),
                office("hq", org_proto::OfficeRole::Hq),
            ]))
            .unwrap();
        assert_eq!(connection.office.id, "hq");

        // No HQ on this machine: the first office it does have.
        let connection = state
            .remember(organization_with(vec![office(
                "sat",
                org_proto::OfficeRole::Satellite,
            )]))
            .unwrap();
        assert_eq!(connection.office.id, "sat");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_organization_without_an_office_here_is_refused_and_not_remembered() {
        let (state, dir) = make_state(Scenario::Healthy);
        let err = state.remember(organization_with(vec![])).unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("app", "no_office"));
        assert!(state.connections().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn hosted(id: &str, name: &str) -> Organization {
        Organization {
            id: id.into(),
            name: name.into(),
            created_at: "2026-09-14T09:30:00.000Z".into(),
            offices: vec![office(&format!("office-{id}"), org_proto::OfficeRole::Hq)],
        }
    }

    fn state_with(organizations: Vec<Organization>) -> (AppState, std::path::PathBuf) {
        let (mut state, dir) = make_state(Scenario::Healthy);
        state.office = Arc::new(
            MockOffice::new(Scenario::Healthy, Duration::ZERO).with_organizations(organizations),
        );
        (state, dir)
    }

    #[test]
    fn existing_organizations_are_listed_and_can_be_opened() {
        let (state, dir) = state_with(vec![hosted("a", "Alpha"), hosted("b", "Beta")]);
        state.ensure_running().unwrap();
        let names: Vec<_> = state
            .organizations()
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(names, ["Alpha", "Beta"]);

        let opened = state.open_organization("b").unwrap();
        assert_eq!(opened.organization.name, "Beta");
        assert_eq!(opened.office.id, "office-b");
        assert_eq!(state.connections().unwrap(), vec![opened]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opening_needs_a_running_office_and_a_known_organization() {
        let (state, dir) = state_with(vec![hosted("a", "Alpha")]);
        let err = state.open_organization("a").unwrap_err();
        assert_eq!(err.code, "not_connected");
        let err = state.organizations().unwrap_err();
        assert_eq!(err.code, "not_connected");

        state.ensure_running().unwrap();
        let err = state.open_organization("missing").unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("app", "not_found"));
        assert!(state.connections().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_organization_whose_office_is_unavailable_is_listed_but_not_opened() {
        let mut down = hosted("d", "Down");
        down.offices[0].status = org_proto::OfficeStatus::Unavailable;
        let (state, dir) = state_with(vec![down, hosted("u", "Up")]);
        state.ensure_running().unwrap();
        assert_eq!(state.organizations().unwrap().len(), 2);
        let err = state.open_organization("d").unwrap_err();
        assert_eq!(
            (err.source, err.code.as_str()),
            ("app", "office_unavailable")
        );
        assert!(state.connections().unwrap().is_empty());
        assert!(state.open_organization("u").is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// An office that fails its first `fail_first` list calls with an office-side error, and counts
    /// how many times it was started.
    struct Flaky {
        starts: std::sync::atomic::AtomicUsize,
        lists_left_to_fail: std::sync::atomic::AtomicUsize,
        /// While set, starting the office fails.
        start_fails: std::sync::atomic::AtomicBool,
    }

    impl OfficeConnection for Flaky {
        fn ensure_running(&self) -> Result<HelloResult, office::SetupError> {
            self.starts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Slow enough that callers which aren't serialized overlap.
            std::thread::sleep(Duration::from_millis(20));
            if self.start_fails.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(office::SetupError {
                    code: office::SetupErrorCode::Unreachable,
                    message: "down".into(),
                });
            }
            Ok(HelloResult {
                api_version: org_proto::API_VERSION,
                server_version: "t".into(),
                host: "This Mac".into(),
                office: None,
            })
        }
        fn create_organization(&self, _: &str) -> Result<Organization, ApiError> {
            unreachable!()
        }
        fn list_organizations(&self) -> Result<Vec<Organization>, ApiError> {
            use std::sync::atomic::Ordering::SeqCst;
            if self.lists_left_to_fail.load(SeqCst) > 0 {
                self.lists_left_to_fail.fetch_sub(1, SeqCst);
                return Err(ApiError::new(
                    org_proto::ErrorCode::Unknown,
                    "the office is gone",
                ));
            }
            Ok(vec![hosted("a", "Alpha")])
        }
        fn org_snapshot(&self, _: &str) -> Result<OrgSnapshot, ApiError> {
            unreachable!()
        }
        fn org_command(&self, _: &str, _: &OrgCommand) -> Result<CommandAccepted, ApiError> {
            unreachable!()
        }
        fn serves_sample_data(&self) -> bool {
            false
        }
    }

    fn flaky_state(failures: usize) -> (AppState, Arc<Flaky>) {
        let flaky = Arc::new(Flaky {
            starts: 0.into(),
            lists_left_to_fail: failures.into(),
            start_fails: false.into(),
        });
        let (mut state, _dir) = make_state(Scenario::Healthy);
        state.office = flaky.clone();
        (state, flaky)
    }

    #[test]
    fn the_office_is_started_once_however_often_it_is_asked_and_restarted_when_it_dies() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        for _ in 0..3 {
            state.start_if_needed_while(|| true).unwrap();
        }
        assert_eq!(
            flaky.starts.load(SeqCst),
            1,
            "asked three times, started once"
        );

        // The office dies: the next list fails once, the app starts it again and succeeds.
        let (state, flaky) = flaky_state(1);
        let listed = state.with_restart(|s| s.organizations()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            flaky.starts.load(SeqCst),
            2,
            "started, then restarted after the failure"
        );

        // Still failing after the restart: the error is reported, not retried forever.
        let (state, flaky) = flaky_state(5);
        assert!(state.with_restart(|s| s.organizations()).is_err());
        assert_eq!(flaky.starts.load(SeqCst), 2);
    }

    #[test]
    fn a_restart_already_done_by_someone_else_is_not_repeated() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        state.start_if_needed_while(|| true).unwrap();
        let seen = state.epoch.load(SeqCst);
        state.restart_since(seen).unwrap();
        assert_eq!(flaky.starts.load(SeqCst), 2, "the first caller restarts it");
        state.restart_since(seen).unwrap();
        assert_eq!(
            flaky.starts.load(SeqCst),
            2,
            "a caller that saw the old failure doesn't restart it again"
        );
    }

    #[test]
    fn a_failed_restart_leaves_the_office_disconnected_until_it_can_be_started() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        state.start_if_needed_while(|| true).unwrap();
        let seen = state.epoch.load(SeqCst);
        flaky.start_fails.store(true, SeqCst);
        assert!(state.restart_since(seen).is_err());
        assert!(
            state.ensure_connected().is_err(),
            "the old connection was forgotten"
        );
        flaky.start_fails.store(false, SeqCst);
        state.start_if_needed_while(|| true).unwrap();
        assert!(state.ensure_connected().is_ok());
    }

    #[test]
    fn a_caller_that_finds_the_office_mid_restart_waits_and_retries() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result = state.with_restart(|_| {
            if calls.fetch_add(1, SeqCst) == 0 {
                Err(CommandError::local("not_connected", "restarting"))
            } else {
                Ok(7)
            }
        });
        assert_eq!(result.unwrap(), 7);
        assert_eq!(calls.load(SeqCst), 2);
        assert_eq!(flaky.starts.load(SeqCst), 1, "waiting is not restarting");
    }

    #[test]
    fn an_answer_from_the_office_is_not_a_reason_to_restart_it() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        let err = state
            .with_restart(|_| -> Result<(), CommandError> {
                Err(ApiError::new(org_proto::ErrorCode::StorageError, "disk").into())
            })
            .unwrap_err();
        assert_eq!((err.source, err.code.as_str()), ("api", "storage_error"));
        assert_eq!(
            flaky.starts.load(SeqCst),
            1,
            "no restart for a storage error"
        );
    }

    #[test]
    fn an_overtaken_request_gives_up_instead_of_starting_the_office() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        let err = state.start_if_needed_while(|| false).unwrap_err();
        assert_eq!(err.code, "superseded");
        assert_eq!(flaky.starts.load(SeqCst), 0);
    }

    #[test]
    fn the_apps_own_failures_are_not_retried() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        let err = state
            .with_restart(|s| s.open_organization("missing"))
            .unwrap_err();
        assert_eq!(err.code, "not_found");
        assert_eq!(
            flaky.starts.load(SeqCst),
            1,
            "no restart for a plain not_found"
        );
    }

    #[test]
    fn concurrent_starts_are_one_at_a_time() {
        use std::sync::atomic::Ordering::SeqCst;
        let (state, flaky) = flaky_state(0);
        let state = Arc::new(state);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let state = state.clone();
                std::thread::spawn(move || state.start_if_needed_while(|| true).unwrap())
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(flaky.starts.load(SeqCst), 1);
    }

    #[test]
    fn an_organization_without_hq_is_refused_when_the_office_it_opens_through_is_unavailable() {
        let mut satellite_first = hosted("s", "Sat");
        satellite_first.offices = vec![
            office("z", org_proto::OfficeRole::Satellite),
            office("a", org_proto::OfficeRole::Satellite),
        ];
        satellite_first.offices[0].status = org_proto::OfficeStatus::Unavailable;
        let (state, dir) = state_with(vec![satellite_first]);
        state.ensure_running().unwrap();
        let err = state.open_organization("s").unwrap_err();
        assert_eq!(err.code, "office_unavailable");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The JSON the commands return is what `proto.ts` and `toAppError` expect. The same fixture is
    /// checked on the TypeScript side (`src/lib/office/shapes.test.ts`).
    #[test]
    fn command_json_matches_the_shared_shapes() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test-vectors/command-shapes.json")).unwrap();
        let error =
            CommandError::from(ApiError::new(org_proto::ErrorCode::AlreadyExists, "Taken."));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            fixture["command_error"]
        );
        let connection: Connection = serde_json::from_value(fixture["connection"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(connection).unwrap(),
            fixture["connection"]
        );
        let view: OrgView = serde_json::from_value(fixture["org_view"].clone()).unwrap();
        assert_eq!(serde_json::to_value(view).unwrap(), fixture["org_view"]);
        let accepted: CommandAccepted =
            serde_json::from_value(fixture["command_accepted"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(accepted).unwrap(),
            fixture["command_accepted"]
        );
    }
}
