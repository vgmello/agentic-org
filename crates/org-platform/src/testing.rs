//! Test adapters built from the contracts, for consumer tests (`test-support`).
use crate::{
    disk::{DiskEncryption, EncryptionState},
    service::{Registration, ServiceError, ServiceManager, ServiceStatus, Unit},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// One recorded service-manager call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Register(Unit),
    Start(String),
    Stop(String),
    Status(String),
    Unregister(String),
    Definition(String),
    Redefine(Unit),
}

/// Records every call and keeps the registrations in memory. A registered unit starts
/// running (as `RunAtLoad` does); it never runs a process.
#[derive(Default)]
pub struct FakeServiceManager {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    /// Each instance's unit and whether it runs.
    units: BTreeMap<String, (Unit, bool)>,
    /// Instances whose definition someone changed by hand ([`FakeServiceManager::edit_by_hand`]).
    edited: BTreeSet<String>,
    calls: Vec<Call>,
}

impl FakeServiceManager {
    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    /// The instance's definition is changed by hand from now on: no call reads it back,
    /// replaces it or registers over it (a conflict), as no adapter takes over a definition
    /// it did not write.
    pub fn edit_by_hand(&self, id: &str) {
        self.state.lock().unwrap().edited.insert(id.to_owned());
    }

    /// The unit registered for `id`, if any, without recording a call.
    pub fn unit(&self, id: &str) -> Option<Unit> {
        let s = self.state.lock().unwrap();
        s.units.get(id).map(|(unit, _)| unit.clone())
    }
}

fn edited(id: &str) -> ServiceError {
    ServiceError::Conflict(format!("the definition of {id} was changed by hand"))
}

impl ServiceManager for FakeServiceManager {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        crate::service::validate(unit)?;
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Register(unit.clone()));
        if s.edited.contains(&unit.instance_id) {
            return Err(edited(&unit.instance_id));
        }
        match s.units.get(&unit.instance_id) {
            Some((existing, _)) if existing != unit => Err(ServiceError::Conflict(
                "another definition uses this instance".into(),
            )),
            Some(_) => Ok(Registration::Unchanged),
            None => {
                s.units
                    .insert(unit.instance_id.clone(), (unit.clone(), true));
                Ok(Registration::Loaded)
            }
        }
    }
    fn start(&self, id: &str) -> Result<(), ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Start(id.into()));
        let entry = s
            .units
            .get_mut(id)
            .ok_or(ServiceError::Invalid("the service is not registered"))?;
        entry.1 = true;
        Ok(())
    }
    fn stop(&self, id: &str) -> Result<(), ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Stop(id.into()));
        if let Some(entry) = s.units.get_mut(id) {
            entry.1 = false;
        }
        Ok(())
    }
    fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Status(id.into()));
        Ok(match s.units.get(id) {
            None => ServiceStatus::NotRegistered,
            Some((_, true)) => ServiceStatus::Running { pid: 1 },
            Some((_, false)) => ServiceStatus::Stopped,
        })
    }
    fn unregister(&self, id: &str) -> Result<(), ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Unregister(id.into()));
        s.units.remove(id);
        Ok(())
    }
    fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
        crate::service::valid_instance_id(id)?;
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Definition(id.into()));
        if s.edited.contains(id) {
            return Err(edited(id));
        }
        Ok(s.units.get(id).map(|(unit, _)| unit.clone()))
    }
    fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        crate::service::validate(unit)?;
        let mut s = self.state.lock().unwrap();
        s.calls.push(Call::Redefine(unit.clone()));
        if s.edited.contains(&unit.instance_id) {
            return Err(edited(&unit.instance_id));
        }
        match s.units.get_mut(&unit.instance_id) {
            None => Err(ServiceError::Invalid("the service is not registered")),
            Some((_, true)) => Err(ServiceError::Conflict(
                "the service is running; stop it before its definition is replaced".into(),
            )),
            Some((existing, false)) if existing == unit => Ok(Registration::Unchanged),
            Some((existing, false)) => {
                *existing = unit.clone();
                Ok(Registration::Loaded)
            }
        }
    }
}

/// Reports a fixed state.
pub struct FakeDiskEncryption(pub EncryptionState);

impl DiskEncryption for FakeDiskEncryption {
    fn state(&self, _path: &Path) -> EncryptionState {
        self.0.clone()
    }
}

/// A service manager that runs the unit as a detached child process instead of registering it
/// with the operating system: real-process tests use it in temporary folders, so no test touches
/// launchd. Its state lives in `state_dir` (the unit's bytes and the child's PID), so a later
/// process sees the same service, as a later `setup` run sees launchd's.
pub struct ChildServiceManager {
    state_dir: PathBuf,
    children: Mutex<BTreeMap<String, std::process::Child>>,
}

impl ChildServiceManager {
    /// `state_dir` must exist and belong to the test.
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            children: Mutex::default(),
        }
    }

    fn unit_file(&self, id: &str) -> PathBuf {
        self.state_dir.join(format!("{id}.unit"))
    }

    fn pid_file(&self, id: &str) -> PathBuf {
        self.state_dir.join(format!("{id}.pid"))
    }

    /// NUL-separated fields: [`crate::service::validate`] refuses control characters, so no
    /// field contains one.
    fn encode(unit: &Unit) -> Vec<u8> {
        let mut fields = vec![
            format!("i:{}", unit.instance_id),
            format!("p:{}", unit.program.to_string_lossy()),
        ];
        fields.extend(
            unit.args
                .iter()
                .map(|a| format!("a:{}", a.to_string_lossy())),
        );
        fields.extend(
            unit.env
                .iter()
                .map(|(k, v)| format!("e:{}={}", k.to_string_lossy(), v.to_string_lossy())),
        );
        fields.join("\0").into_bytes()
    }

    fn decode(bytes: &[u8]) -> Option<Unit> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut unit = Unit {
            instance_id: String::new(),
            program: PathBuf::new(),
            args: Vec::new(),
            env: Vec::new(),
        };
        for field in text.split('\0') {
            match field.split_once(':')? {
                ("i", id) => unit.instance_id = id.to_owned(),
                ("p", program) => unit.program = program.into(),
                ("a", arg) => unit.args.push(arg.into()),
                ("e", pair) => {
                    let (k, v) = pair.split_once('=')?;
                    unit.env.push((k.into(), v.into()));
                }
                _ => return None,
            }
        }
        Some(unit)
    }

    fn pid(&self, id: &str) -> Option<u32> {
        let pid: u32 = std::fs::read_to_string(self.pid_file(id))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        // Reap our own exited child first, so a zombie does not look alive.
        if let Some(child) = self.children.lock().unwrap().get_mut(id) {
            if child.id() == pid && !matches!(child.try_wait(), Ok(None)) {
                return None;
            }
        }
        let pid_t = libc::pid_t::try_from(pid).ok()?;
        // SAFETY: signal 0 only checks that the process exists.
        (unsafe { libc::kill(pid_t, 0) } == 0).then_some(pid)
    }

    fn system(why: impl std::fmt::Display) -> ServiceError {
        ServiceError::System(why.to_string())
    }
}

impl ServiceManager for ChildServiceManager {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        crate::service::validate(unit)?;
        let bytes = Self::encode(unit);
        match std::fs::read(self.unit_file(&unit.instance_id)) {
            Ok(existing) if existing == bytes => Ok(Registration::Unchanged),
            Ok(_) => Err(ServiceError::Conflict(
                "another definition uses this instance".into(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::write(self.unit_file(&unit.instance_id), bytes).map_err(Self::system)?;
                Ok(Registration::Loaded)
            }
            Err(e) => Err(Self::system(e)),
        }
    }

    fn start(&self, id: &str) -> Result<(), ServiceError> {
        crate::service::valid_instance_id(id)?;
        if self.pid(id).is_some() {
            return Ok(());
        }
        let bytes = std::fs::read(self.unit_file(id))
            .map_err(|_| ServiceError::Invalid("the service is not registered"))?;
        let unit =
            Self::decode(&bytes)
                .filter(|u| u.instance_id == id)
                .ok_or(ServiceError::Invalid(
                    "the registered definition is unreadable",
                ))?;
        let log = std::fs::File::create(self.state_dir.join(format!("{id}.log")))
            .map_err(Self::system)?;
        let mut command = std::process::Command::new(&unit.program);
        command
            .args(&unit.args)
            .env_clear()
            .envs(unit.env.iter().map(|(k, v)| (k, v)))
            .current_dir("/")
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone().map_err(Self::system)?)
            .stderr(log);
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let child = command.spawn().map_err(Self::system)?;
        std::fs::write(self.pid_file(id), child.id().to_string()).map_err(Self::system)?;
        self.children.lock().unwrap().insert(id.to_owned(), child);
        Ok(())
    }

    fn stop(&self, id: &str) -> Result<(), ServiceError> {
        crate::service::valid_instance_id(id)?;
        let Some(pid) = self.pid(id) else {
            return Ok(());
        };
        let pid_t = libc::pid_t::try_from(pid).map_err(Self::system)?;
        // SAFETY: plain signal to a PID this manager started.
        unsafe { libc::kill(pid_t, libc::SIGTERM) };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while self.pid(id).is_some() {
            if std::time::Instant::now() >= deadline {
                // SAFETY: as above.
                unsafe { libc::kill(pid_t, libc::SIGKILL) };
                return Err(ServiceError::System("the service did not stop".into()));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = std::fs::remove_file(self.pid_file(id));
        Ok(())
    }

    fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
        crate::service::valid_instance_id(id)?;
        if !self.unit_file(id).exists() {
            return Ok(ServiceStatus::NotRegistered);
        }
        Ok(match self.pid(id) {
            Some(pid) => ServiceStatus::Running { pid },
            None => ServiceStatus::Stopped,
        })
    }

    fn unregister(&self, id: &str) -> Result<(), ServiceError> {
        self.stop(id)?;
        let _ = std::fs::remove_file(self.unit_file(id));
        Ok(())
    }

    fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
        crate::service::valid_instance_id(id)?;
        match std::fs::read(self.unit_file(id)) {
            Ok(bytes) => Self::decode(&bytes)
                .filter(|u| u.instance_id == id)
                .map(Some)
                .ok_or_else(|| {
                    ServiceError::Conflict("the registered definition is not this manager's".into())
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Self::system(e)),
        }
    }

    fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        crate::service::validate(unit)?;
        if self.pid(&unit.instance_id).is_some() {
            return Err(ServiceError::Conflict(
                "the service is running; stop it before its definition is replaced".into(),
            ));
        }
        let Some(existing) = self.definition(&unit.instance_id)? else {
            return Err(ServiceError::Invalid("the service is not registered"));
        };
        if existing == *unit {
            return Ok(Registration::Unchanged);
        }
        std::fs::write(self.unit_file(&unit.instance_id), Self::encode(unit))
            .map_err(Self::system)?;
        Ok(Registration::Loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_child_manager_runs_stops_and_refuses_a_different_definition() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ChildServiceManager::new(dir.path());
        let unit = Unit {
            instance_id: "abc".into(),
            program: "/bin/sleep".into(),
            args: vec!["30".into()],
            env: vec![("A".into(), "b=c".into())],
        };
        assert_eq!(
            ChildServiceManager::decode(&ChildServiceManager::encode(&unit)),
            Some(unit.clone())
        );
        assert_eq!(manager.status("abc").unwrap(), ServiceStatus::NotRegistered);
        assert_eq!(manager.register(&unit).unwrap(), Registration::Loaded);
        assert_eq!(manager.register(&unit).unwrap(), Registration::Unchanged);
        let other = Unit {
            args: vec!["31".into()],
            ..unit.clone()
        };
        assert!(matches!(
            manager.register(&other),
            Err(ServiceError::Conflict(_))
        ));
        manager.start("abc").unwrap();
        let ServiceStatus::Running { pid } = manager.status("abc").unwrap() else {
            panic!("started");
        };
        // A second manager over the same folder (a later process) sees the same child, and a
        // second start does not start another.
        let later = ChildServiceManager::new(dir.path());
        later.start("abc").unwrap();
        assert_eq!(later.status("abc").unwrap(), ServiceStatus::Running { pid });
        manager.stop("abc").unwrap();
        assert_eq!(manager.status("abc").unwrap(), ServiceStatus::Stopped);
    }

    #[test]
    fn the_fake_manager_is_idempotent_and_refuses_a_different_definition() {
        let fake = FakeServiceManager::default();
        let unit = Unit {
            instance_id: "abc".into(),
            program: "/bin/x".into(),
            args: vec![],
            env: vec![],
        };
        assert_eq!(fake.register(&unit).unwrap(), Registration::Loaded);
        assert_eq!(fake.register(&unit).unwrap(), Registration::Unchanged);
        let other = Unit {
            args: vec!["y".into()],
            ..unit.clone()
        };
        assert!(matches!(
            fake.register(&other),
            Err(ServiceError::Conflict(_))
        ));
        fake.stop("abc").unwrap();
        assert_eq!(fake.status("abc").unwrap(), ServiceStatus::Stopped);
        assert_eq!(fake.calls()[0], Call::Register(unit));
    }

    /// The fakes read a definition back, replace it only while it is stopped and never
    /// start it, and refuse one changed by hand (as the adapters do).
    #[test]
    fn the_fakes_read_back_and_redefine_only_a_stopped_definition_of_their_own() {
        let unit = Unit {
            instance_id: "abc".into(),
            program: "/bin/sleep".into(),
            args: vec!["30".into()],
            env: vec![],
        };
        let upgraded = Unit {
            program: "/bin/cat".into(),
            ..unit.clone()
        };
        let fake = FakeServiceManager::default();
        assert_eq!(fake.definition("abc").unwrap(), None);
        assert!(matches!(
            fake.redefine(&unit),
            Err(ServiceError::Invalid(_))
        ));
        fake.register(&unit).unwrap();
        assert_eq!(fake.definition("abc").unwrap(), Some(unit.clone()));
        assert!(matches!(
            fake.redefine(&upgraded),
            Err(ServiceError::Conflict(_))
        ));
        fake.stop("abc").unwrap();
        assert_eq!(fake.redefine(&upgraded).unwrap(), Registration::Loaded);
        assert_eq!(fake.redefine(&upgraded).unwrap(), Registration::Unchanged);
        assert_eq!(fake.status("abc").unwrap(), ServiceStatus::Stopped);
        assert_eq!(fake.unit("abc"), Some(upgraded.clone()));
        fake.edit_by_hand("abc");
        for refused in [
            fake.definition("abc").map(drop),
            fake.redefine(&unit).map(drop),
            fake.register(&upgraded).map(drop),
        ] {
            assert!(matches!(refused, Err(ServiceError::Conflict(_))));
        }

        let dir = tempfile::tempdir().unwrap();
        let child = ChildServiceManager::new(dir.path());
        assert_eq!(child.definition("abc").unwrap(), None);
        child.register(&unit).unwrap();
        assert_eq!(child.definition("abc").unwrap(), Some(unit.clone()));
        child.start("abc").unwrap();
        assert!(matches!(
            child.redefine(&upgraded),
            Err(ServiceError::Conflict(_))
        ));
        child.stop("abc").unwrap();
        assert_eq!(child.redefine(&upgraded).unwrap(), Registration::Loaded);
        assert_eq!(child.status("abc").unwrap(), ServiceStatus::Stopped);
        assert_eq!(child.definition("abc").unwrap(), Some(upgraded));
        std::fs::write(dir.path().join("abc.unit"), b"edited").unwrap();
        assert!(matches!(
            child.definition("abc"),
            Err(ServiceError::Conflict(_))
        ));
    }
}
