//! The menu-bar item: organizations first, each with its offices (UX spec §4). It informs and
//! switches; everything else happens in the window. The menu's content is decided in
//! `menu_model.rs`; this file turns it into a native menu and handles clicks.

use crate::menu_model::{self, Menu as Model, OrganizationEntry};
use crate::AppState;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "main";
/// The newest refresh. A refresh that finishes after a newer one started drops its result, so an
/// older, slower answer never overwrites a newer menu.
static LATEST: AtomicU64 = AtomicU64::new(0);
/// Held while a menu is built and set, so the generation check and the set are one step.
static APPLYING: Mutex<()> = Mutex::new(());

const ICON: &[u8] = include_bytes!("../icons/tray.png");

/// Whether the app shows in the Dock. Kept in the app's data folder.
pub struct DockPreference {
    file: PathBuf,
    show: Mutex<bool>,
}

impl DockPreference {
    pub fn load(dir: PathBuf) -> Self {
        let file = dir.join("preferences.json");
        let show = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("show_in_dock").and_then(|s| s.as_bool()))
            .unwrap_or(true);
        Self {
            file,
            show: Mutex::new(show),
        }
    }

    pub fn shown(&self) -> bool {
        *self.show.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Flip it, save it (temp file then rename), and return the new value.
    fn toggle(&self) -> bool {
        let mut show = self.show.lock().unwrap_or_else(|e| e.into_inner());
        *show = !*show;
        let json = serde_json::json!({ "show_in_dock": *show });
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.file.with_extension("json.tmp");
        if std::fs::write(&tmp, json.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.file);
        }
        *show
    }
}

#[cfg(target_os = "macos")]
pub fn apply_dock(app: &AppHandle, show: bool) {
    let policy = if show {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    let _ = app.set_activation_policy(policy);
}

#[cfg(not(target_os = "macos"))]
pub fn apply_dock(_app: &AppHandle, _show: bool) {}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let icon = Image::from_bytes(ICON)?;
    let menu = native_menu(app, None)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Agentic Org")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle(app, event.id().as_ref()))
        .build(app)?;
    refresh(app);
    Ok(())
}

/// Bring the window forward, wherever it is.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Show the window and ask the UI to go to `path`.
pub fn show_and_navigate(app: &AppHandle, path: &str) {
    show_window(app);
    let _ = app.emit("navigate", path);
}

/// Rebuild the menu from the office's current organizations, off the main thread (asking the
/// office can take a moment).
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    let generation = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let state = Arc::clone(&app.state::<Arc<AppState>>());
        let wanted = || LATEST.load(Ordering::SeqCst) == generation;
        let model = match state.with_restart_while(wanted, |s| s.organizations()) {
            // An older refresh that was overtaken while waiting: the newer one will show the menu.
            Err(e) if e.code == "superseded" => return,
            // The host couldn't be reached or started.
            Err(e) if e.source == "setup" => Model::OfficesNotRunning,
            Err(_) => Model::Unreadable,
            Ok(organizations) => {
                let current = state
                    .connections()
                    .ok()
                    .and_then(|c| c.last().map(|c| c.organization.id.clone()));
                menu_model::build(&organizations, current.as_deref())
            }
        };
        let _applying = APPLYING.lock().unwrap_or_else(|e| e.into_inner());
        if LATEST.load(Ordering::SeqCst) != generation {
            return; // a newer refresh is running; its answer is the one to show
        }
        if let (Ok(menu), Some(tray)) = (native_menu(&app, Some(&model)), app.tray_by_id(TRAY_ID)) {
            let _ = tray.set_menu(Some(menu));
        }
    });
}

fn native_menu(app: &AppHandle, model: Option<&Model>) -> tauri::Result<Menu<tauri::Wry>> {
    let mut menu = MenuBuilder::new(app);
    let disabled = |text: &str| MenuItemBuilder::new(text).enabled(false).build(app);

    match model {
        None => menu = menu.item(&disabled("Loading organizations…")?),
        Some(Model::OfficesNotRunning) => {
            menu = menu.item(&disabled("Offices aren't running")?).item(
                &MenuItemBuilder::with_id("login-items", "Open Login Items settings…")
                    .build(app)?,
            );
        }
        Some(Model::Unreadable) => menu = menu.item(&disabled("Couldn't read the organizations")?),
        Some(Model::Organizations(entries)) if entries.is_empty() => {
            menu = menu.item(&disabled("No organizations on this Mac yet")?)
        }
        Some(Model::Organizations(entries)) => {
            for entry in entries {
                match entry {
                    OrganizationEntry::Available {
                        id,
                        name,
                        current,
                        shortcut,
                        offices,
                        more_offices,
                    } => {
                        let mut item =
                            CheckMenuItemBuilder::with_id(menu_model::organization_item(id), name)
                                .checked(*current);
                        if let Some(n) = shortcut {
                            item = item.accelerator(format!("CmdOrCtrl+{n}"));
                        }
                        menu = menu.item(&item.build(app)?);
                        for office in offices {
                            let text = format!("    {}", office.label);
                            menu = menu.item(
                                &MenuItemBuilder::with_id(
                                    menu_model::office_item(id, &office.id),
                                    text,
                                )
                                .build(app)?,
                            );
                        }
                        if *more_offices > 0 {
                            let noun = if *more_offices == 1 {
                                "office"
                            } else {
                                "offices"
                            };
                            let text = format!("    {more_offices} more {noun}…");
                            menu = menu.item(
                                &MenuItemBuilder::with_id(menu_model::offices_item(id), text)
                                    .build(app)?,
                            );
                        }
                    }
                    OrganizationEntry::Unavailable { name, reason, .. } => {
                        menu = menu
                            .item(&disabled(name)?)
                            .item(&disabled(&format!("    {reason}"))?);
                    }
                }
            }
        }
    }

    let dock_shown = app
        .try_state::<DockPreference>()
        .map(|p| p.shown())
        .unwrap_or(true);
    menu.separator()
        .item(
            &MenuItemBuilder::with_id("open", "Open Agentic Org")
                .accelerator("CmdOrCtrl+O")
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id("open-join", "Open or join organization…").build(app)?)
        .item(&MenuItemBuilder::with_id("create", "Create organization…").build(app)?)
        .separator()
        .item(
            &CheckMenuItemBuilder::with_id("dock", "Show in Dock")
                .checked(dock_shown)
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("quit", "Quit Agentic Org")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?,
        )
        .build()
}

fn handle(app: &AppHandle, id: &str) {
    match id {
        "open" => show_window(app),
        "open-join" => show_and_navigate(app, "/welcome"),
        "create" => show_and_navigate(app, "/create"),
        "dock" => {
            if let Some(preference) = app.try_state::<DockPreference>() {
                apply_dock(app, preference.toggle());
            }
            refresh(app);
        }
        "login-items" => {
            // A fixed System Settings address, nothing from the menu or the office.
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")
                .spawn();
        }
        "quit" => app.exit(0),
        other => match menu_model::parse_item(other) {
            Some(menu_model::ItemId::Organization(id)) => switch_to(app, id, "/agents"),
            // The office's own details come with the Offices screen; for now, its organization's.
            Some(menu_model::ItemId::Office { organization, .. }) => {
                switch_to(app, organization, "/offices")
            }
            Some(menu_model::ItemId::Offices(organization)) => {
                switch_to(app, organization, "/offices")
            }
            None => {}
        },
    }
}

/// Make `organization_id` the current organization and bring the window to `path`, without a
/// confirmation (UX spec MENU-2). If it can't be opened, show the Open organization screen.
fn switch_to(app: &AppHandle, organization_id: &str, path: &'static str) {
    let app = app.clone();
    let id = organization_id.to_string();
    // The window comes forward at once; the menu click shouldn't look ignored while the office answers.
    show_window(&app);
    std::thread::spawn(move || {
        let state = Arc::clone(&app.state::<Arc<AppState>>());
        let opened = state.with_restart(|s| s.open_organization(&id)).is_ok();
        let _ = app.emit("navigate", if opened { path } else { "/welcome" });
        refresh(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agentic-org-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_dock_is_shown_by_default_and_the_choice_is_kept_between_launches() {
        let dir = temp("dock");
        assert!(DockPreference::load(dir.clone()).shown(), "default: shown");
        let first = DockPreference::load(dir.clone());
        assert!(!first.toggle(), "hidden after the first toggle");
        assert!(
            !DockPreference::load(dir.clone()).shown(),
            "kept across a restart"
        );
        assert!(first.toggle(), "shown again");
        assert!(DockPreference::load(dir.clone()).shown());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_preference_file_means_the_default() {
        let dir = temp("dock-corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("preferences.json"), b"{ not json").unwrap();
        assert!(DockPreference::load(dir.clone()).shown());
        std::fs::write(dir.join("preferences.json"), br#"{"show_in_dock": "yes"}"#).unwrap();
        assert!(DockPreference::load(dir.clone()).shown());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
