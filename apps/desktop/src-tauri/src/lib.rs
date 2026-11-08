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
use org_proto::{ApiError, HelloResult, OfficeInfo};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

/// What the shell holds for the UI commands.
struct AppState {
    office: Arc<dyn OfficeConnection>,
    store: ConnectionStore,
    /// The office the last successful `ensure_running` reached.
    connected: Mutex<Option<OfficeInfo>>,
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
    fn ensure_running(&self) -> Result<HelloResult, CommandError> {
        let hello = self.office.ensure_running()?;
        *self.connected.lock().unwrap_or_else(|e| e.into_inner()) = Some(hello.office.clone());
        Ok(hello)
    }

    /// Create the organization on the connected office and remember the connection.
    ///
    /// Known gap: if the office creates the organization but `remember` then fails (or the app
    /// is closed in between), the organization exists on the office and the app doesn't know it.
    /// A retry then gets `already_exists`. The real implementation must let the owner attach to
    /// an existing organization (the "Open organization" flow).
    fn create_organization(&self, name: &str) -> Result<Connection, CommandError> {
        let office = self
            .connected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| CommandError::local("not_connected", "The office isn't running yet."))?;
        let organization = self.office.create_organization(name)?;
        let connection = Connection {
            office,
            organization,
        };
        self.store
            .remember(connection.clone())
            .map_err(|e| CommandError::local("remember_failed", e.to_string()))?;
        Ok(connection)
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
                office: Arc::new(mock::MockOffice::from_env()),
                store: ConnectionStore::new(dir),
                connected: Mutex::new(None),
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            office_ensure_running,
            organization_create,
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

    const COMMANDS: [&str; 3] = [
        "office_ensure_running",
        "organization_create",
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
