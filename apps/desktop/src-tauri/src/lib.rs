//! agentic.org: the desktop client. It talks to an office server through `OfficeConnection` and
//! never reads an office's storage itself.
//!
//! Until `org-client` and the office-setup layer exist, the office is `mock::MockOffice`, in
//! every build, so the shell can be run and shown end to end. It is wired in one place (`run`).

mod connections;
mod mock;
mod office;

use connections::{Connection, ConnectionStore};
use office::{OfficeConnection, SetupError};
use org_proto::{ApiError, HelloResult, OfficeInfo, OfficeRole, OfficeStatus, Organization};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

/// What the shell holds for the UI commands.
struct AppState {
    office: Arc<dyn OfficeConnection>,
    store: ConnectionStore,
    /// The host the last successful `ensure_running` reached ("This Mac").
    connected: Mutex<Option<String>>,
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

/// The office an organization is reached through on this machine: its HQ, else its first office.
fn local_office(organization: &Organization) -> Option<&OfficeInfo> {
    organization
        .offices
        .iter()
        .find(|o| o.role == OfficeRole::Hq)
        .or_else(|| organization.offices.first())
}

impl AppState {
    fn ensure_running(&self) -> Result<HelloResult, CommandError> {
        let hello = self.office.ensure_running()?;
        *self.connected.lock().unwrap_or_else(|e| e.into_inner()) = Some(hello.host.clone());
        Ok(hello)
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
    state: State<'_, Arc<AppState>>,
    name: String,
) -> Result<Connection, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.create_organization(&name)).await
}

#[tauri::command]
async fn organizations_list(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Organization>, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.organizations()).await
}

#[tauri::command]
async fn organization_open(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Connection, CommandError> {
    let state = Arc::clone(&state);
    blocking(move || state.open_organization(&id)).await
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
            let dir = app.path().app_data_dir()?.join("mock");
            app.manage(Arc::new(AppState {
                office: Arc::new(mock::MockOffice::from_env(dir.clone())),
                store: ConnectionStore::new(dir),
                connected: Mutex::new(None),
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            office_ensure_running,
            organization_create,
            organizations_list,
            organization_open,
            connections_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running agentic.org");
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
        };
        (state, dir)
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

    const COMMANDS: [&str; 5] = [
        "office_ensure_running",
        "organization_create",
        "organizations_list",
        "organization_open",
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
    }
}
