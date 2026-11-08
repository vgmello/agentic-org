//! The office host as a login service: a LaunchAgent that ships inside the app bundle and is registered with
//! macOS (`SMAppService`), so the host starts with the Mac and keeps running when the app is closed.
//!
//! This module is the app's whole knowledge of that service: what state it is in, how to turn it on or off, and
//! how to send the owner to System Settings when macOS wants their approval. `RealOffice` uses it to start the host
//! when nothing answers on the host socket.

use serde::Serialize;

/// The file name of the LaunchAgent plist in `Contents/Library/LaunchAgents/` of the app bundle.
pub const PLIST_NAME: &str = "org.agentic.office-host.plist";

/// What macOS says about the login service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    /// Not registered yet (first launch, or turned off in Settings).
    NotRegistered,
    /// Registered and allowed: macOS keeps the host running.
    Enabled,
    /// Registered, but the owner has to allow it in System Settings → General → Login Items.
    RequiresApproval,
    /// macOS can't find the service: this copy of the app doesn't contain the plist (a development build run
    /// outside a bundle, or a broken install).
    NotFound,
}

impl LoginState {
    /// From the raw `SMAppServiceStatus` value (0 not registered, 1 enabled, 2 requires approval, 3 not found).
    /// A value from a newer macOS is treated as not found: the safe reading is "can't use it".
    pub fn from_raw(raw: i64) -> Self {
        match raw {
            0 => Self::NotRegistered,
            1 => Self::Enabled,
            2 => Self::RequiresApproval,
            _ => Self::NotFound,
        }
    }
}

/// The login service the host runs under.
pub trait LoginService: Send + Sync {
    fn status(&self) -> LoginState;
    /// Register the service (macOS may then ask the owner to approve it). The reason, in words, when macOS refuses.
    fn register(&self) -> Result<(), String>;
    fn unregister(&self) -> Result<(), String>;
    /// Open System Settings at Login Items.
    fn open_settings(&self);
    /// The owner turned the service off on purpose (Settings): the app must not turn it on again by itself.
    fn turned_off(&self) -> bool {
        false
    }
}

/// A login service whose "off" is remembered: turning it off leaves a marker file in the app's data folder, turning it on
/// removes it. The app starts the host itself only the first time and whenever the owner has not turned it off.
pub struct Remembered {
    inner: Box<dyn LoginService>,
    marker: std::path::PathBuf,
}

impl Remembered {
    pub fn new(inner: Box<dyn LoginService>, data_dir: &std::path::Path) -> Self {
        Self {
            inner,
            marker: data_dir.join("login-item-off"),
        }
    }
}

impl LoginService for Remembered {
    fn status(&self) -> LoginState {
        self.inner.status()
    }
    fn register(&self) -> Result<(), String> {
        let result = self.inner.register();
        // On again, whether or not macOS wants approval first: the owner asked for it.
        if result.is_ok() || self.inner.status() != LoginState::NotRegistered {
            let _ = std::fs::remove_file(&self.marker);
        }
        result
    }
    fn unregister(&self) -> Result<(), String> {
        let result = self.inner.unregister();
        if result.is_ok() || self.inner.status() == LoginState::NotRegistered {
            let _ = std::fs::write(&self.marker, b"off");
        }
        result
    }
    fn open_settings(&self) {
        self.inner.open_settings()
    }
    fn turned_off(&self) -> bool {
        self.marker.exists()
    }
}

/// For a platform without the service (and for tests that don't care): nothing can be installed.
#[cfg(any(test, not(target_os = "macos")))]
pub struct Unsupported;

#[cfg(any(test, not(target_os = "macos")))]
impl LoginService for Unsupported {
    fn status(&self) -> LoginState {
        LoginState::NotFound
    }
    fn register(&self) -> Result<(), String> {
        Err("Login items aren't available on this system.".into())
    }
    fn unregister(&self) -> Result<(), String> {
        Err("Login items aren't available on this system.".into())
    }
    fn open_settings(&self) {}
}

/// The service for this platform, with the owner's "off" remembered in `data_dir`.
pub fn system_in(data_dir: &std::path::Path) -> Box<dyn LoginService> {
    Box::new(Remembered::new(system(), data_dir))
}

/// The service for this platform.
pub fn system() -> Box<dyn LoginService> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::SmAppService)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(Unsupported)
    }
}

/// A login service that moves through scripted states, for tests: each `status()` returns the next state (the
/// last one repeats), and `register()` runs `on_register` and answers `register_result`.
#[cfg(test)]
pub struct FakeLogin {
    states: std::sync::Mutex<std::collections::VecDeque<LoginState>>,
    pub register_calls: std::sync::atomic::AtomicUsize,
    pub unregister_calls: std::sync::atomic::AtomicUsize,
    pub settings_opened: std::sync::atomic::AtomicUsize,
    pub register_result: Result<(), String>,
    pub off: bool,
    pub on_register: Box<dyn Fn() + Send + Sync>,
}

#[cfg(test)]
impl FakeLogin {
    pub fn new(states: &[LoginState]) -> Self {
        Self {
            states: std::sync::Mutex::new(states.iter().copied().collect()),
            register_calls: Default::default(),
            unregister_calls: Default::default(),
            settings_opened: Default::default(),
            register_result: Ok(()),
            off: false,
            on_register: Box::new(|| {}),
        }
    }
}

#[cfg(test)]
impl LoginService for FakeLogin {
    fn status(&self) -> LoginState {
        let mut states = self.states.lock().unwrap();
        if states.len() > 1 {
            states.pop_front().unwrap()
        } else {
            *states.front().unwrap()
        }
    }
    fn register(&self) -> Result<(), String> {
        self.register_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (self.on_register)();
        self.register_result.clone()
    }
    fn unregister(&self) -> Result<(), String> {
        self.unregister_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn open_settings(&self) {
        self.settings_opened
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    fn turned_off(&self) -> bool {
        self.off
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{LoginService, LoginState, PLIST_NAME};
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use objc2_service_management::SMAppService;

    pub struct SmAppService;

    fn service() -> Retained<SMAppService> {
        let plist = NSString::from_str(PLIST_NAME);
        // SAFETY: `plist` is a valid NSString that lives for the call; the class method has no other requirements.
        unsafe { SMAppService::agentServiceWithPlistName(&plist) }
    }

    impl LoginService for SmAppService {
        fn status(&self) -> LoginState {
            // SAFETY: a plain getter on a valid service object.
            LoginState::from_raw(unsafe { service().status() }.0 as i64)
        }

        fn register(&self) -> Result<(), String> {
            // SAFETY: a valid service object; the error comes back as a value.
            unsafe { service().registerAndReturnError() }
                .map_err(|e| e.localizedDescription().to_string())
        }

        fn unregister(&self) -> Result<(), String> {
            // SAFETY: as above.
            unsafe { service().unregisterAndReturnError() }
                .map_err(|e| e.localizedDescription().to_string())
        }

        fn open_settings(&self) {
            // SAFETY: a class method with no arguments.
            unsafe { SMAppService::openSystemSettingsLoginItems() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raw_status_maps_to_a_state_and_an_unknown_one_is_not_found() {
        assert_eq!(LoginState::from_raw(0), LoginState::NotRegistered);
        assert_eq!(LoginState::from_raw(1), LoginState::Enabled);
        assert_eq!(LoginState::from_raw(2), LoginState::RequiresApproval);
        assert_eq!(LoginState::from_raw(3), LoginState::NotFound);
        assert_eq!(LoginState::from_raw(99), LoginState::NotFound);
        assert_eq!(LoginState::from_raw(-1), LoginState::NotFound);
    }

    #[test]
    fn states_are_sent_to_the_ui_in_snake_case() {
        let word = |s| serde_json::to_string(&s).unwrap();
        assert_eq!(word(LoginState::RequiresApproval), "\"requires_approval\"");
        assert_eq!(word(LoginState::NotRegistered), "\"not_registered\"");
    }

    /// The plist that ships in the bundle is the one the app registers: same file name, same label, and it starts
    /// the host from where the bundle puts it.
    #[test]
    fn the_shipped_plist_names_the_host_and_the_app() {
        let plist = include_str!("../launch-agents/org.agentic.office-host.plist");
        assert!(plist.contains("<string>org.agentic.office-host</string>"));
        assert!(plist.contains("<string>Contents/MacOS/org-office-host</string>"));
        assert!(plist.contains("<string>com.ellosoft.agentic-org</string>"));
        assert!(
            !plist.contains("APP_BUNDLE_ID"),
            "the placeholder is filled in"
        );
        assert_eq!(PLIST_NAME, "org.agentic.office-host.plist");
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], "com.ellosoft.agentic-org");
        // The bundle puts the plist, the host and the server where the plist and the host expect them.
        let files = &conf["bundle"]["macOS"]["files"];
        assert_eq!(
            files[format!("Library/LaunchAgents/{PLIST_NAME}")],
            format!("launch-agents/{PLIST_NAME}")
        );
        assert_eq!(files["MacOS/org-office-host"], "office-bin/org-office-host");
        assert_eq!(
            files["MacOS/org-office-server"],
            "office-bin/org-office-server"
        );
        assert!(std::path::Path::new(&format!("launch-agents/{PLIST_NAME}")).exists());
    }

    #[test]
    fn off_is_remembered_until_the_owner_turns_it_on() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeLogin::new(&[
            LoginState::Enabled,
            LoginState::NotRegistered,
            LoginState::NotRegistered,
            LoginState::Enabled,
        ]);
        let service = Remembered::new(Box::new(fake), dir.path());
        assert!(!service.turned_off(), "a first run is not off");
        service.unregister().unwrap();
        assert!(service.turned_off());
        // Remembered across launches: a new wrapper over the same folder sees it.
        let again = Remembered::new(
            Box::new(FakeLogin::new(&[LoginState::NotRegistered])),
            dir.path(),
        );
        assert!(again.turned_off());
        service.register().unwrap();
        assert!(!service.turned_off());
    }
}
