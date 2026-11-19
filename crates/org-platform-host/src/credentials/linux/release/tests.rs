//! Tests of the release directory against real directory trees.
//!
//! Two kinds:
//!
//! - **From `/`**, through the contract ([`release_directory`]), exactly as a host
//!   opens its store. A test that needs the store to be *accepted* needs a directory
//!   whose every ancestor passes the ancestor rule, which a shared temporary directory
//!   (`/tmp`, `/var/tmp`) never does: it is writable by others. Such a test uses
//!   cargo's own test directory (`target/tmp`), and prints why it could not run where
//!   that directory's ancestors do not pass either.
//! - **From a directory the test made** ([`Tree`]): the same walk and the same checks,
//!   started at that directory instead of at `/` ([`release_directory_below`], the
//!   entry the host's tests use too), so that every rule runs on a real tree below it
//!   whatever is above it. The directories above the test's own are not judged in
//!   these tests; the tests from `/` judge them.
//!
//! The tests run as whatever user runs them. Where a case needs a file of another
//! user, only root can make one: such a case says so and runs only then; the pure
//! decision (`rules.rs`) covers it everywhere.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::*;
use crate::credentials::{
    release_directory_with_stores as contract, Credentials, FilesystemClass, ItemName, Sharing,
    MAX_ITEM_BYTES,
};
// The contract's handle: what a host holds. The tests wrap the adapter's own.
use crate::credentials::ReleaseRoot as Opened;

/// The host's two item stores, as `org-host` names them.
const OFFICE_KEYS: ReleaseItems<'static> = ReleaseItems {
    directory: "office-keys",
    suffix: ".key",
    sharing: Sharing::Exclusive,
};
const HOST_SECRETS: ReleaseItems<'static> = ReleaseItems {
    directory: "host-secrets",
    suffix: "",
    sharing: Sharing::Exclusive,
};

/// The host's own name rules stand in here as two simple ones: an office key's name is
/// anything but `not-an-office`, a host secret's anything but `README`.
fn is_office(name: &str) -> bool {
    name != "not-an-office"
}
fn is_host_secret(name: &str) -> bool {
    name != "README"
}

/// The two item stores as the host declares them when it opens the directory.
const STORES: [ItemStore<'static>; 2] = [
    ItemStore {
        directory: "office-keys",
        suffix: ".key",
        is_item: is_office,
    },
    ItemStore {
        directory: "host-secrets",
        suffix: "",
        is_item: is_host_secret,
    },
];

fn euid() -> u32 {
    // SAFETY: no arguments and no side effects; it cannot fail.
    unsafe { libc::geteuid() }
}

fn is_root() -> bool {
    euid() == 0
}

fn mkdir(path: &Path, mode: u32) {
    fs::create_dir(path).unwrap();
    chmod(path, mode);
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & 0o7777
}

fn name(raw: &str) -> ItemName {
    ItemName::new(raw).unwrap()
}

fn loaded(store: &Credentials, item: &str) -> Option<Vec<u8>> {
    store
        .load(&name(item))
        .unwrap()
        .map(|secret| secret.expose().to_vec())
}

/// Every entry below `dir`, as paths relative to it, sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let relative = prefix.join(entry.file_name());
            found.push(relative.to_string_lossy().into_owned());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &relative, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, Path::new(""), &mut found);
    found.sort();
    found
}

/// The rule and the path a refused open named.
fn refusal<T>(result: Result<T, CredentialError>) -> (RootRule, PathBuf) {
    match result {
        Err(CredentialError::Root(RootRefusal { rule, path })) => (rule, path),
        Err(other) => panic!("refused with {other:?}, not with a rule of the release directory"),
        Ok(_) => panic!("the release directory, or a folder in it, was opened"),
    }
}

/// A group this user may give a directory that is not this user's private group, if
/// there is one: root may give any, another user one of its supplementary groups.
fn another_group() -> Option<u32> {
    let private = account::private_group_of(euid()).unwrap();
    if is_root() {
        return [1, 2, 3].into_iter().find(|group| Some(*group) != private);
    }
    let mut groups = vec![0 as libc::gid_t; 256];
    // SAFETY: a buffer of the length passed; the call writes at most that many ids.
    let count = unsafe { libc::getgroups(groups.len() as libc::c_int, groups.as_mut_ptr()) };
    groups.truncate(usize::try_from(count).ok()?);
    // SAFETY: no arguments and no side effects; it cannot fail.
    let primary = unsafe { libc::getegid() };
    groups
        .into_iter()
        .find(|group| *group != primary && Some(*group) != private)
}

fn chgrp(path: &Path, group: u32) {
    std::os::unix::fs::chown(path, None, Some(group)).unwrap();
}

/// A tree of real directories in a temporary directory, and the start of the walks
/// into it.
struct Tree {
    _temp: tempfile::TempDir,
    /// The directory the walk starts at: this user's, mode `0700`, resolved.
    base: PathBuf,
    /// A data root next to everything a test makes: `<base>/data`, mode `0700`.
    data: PathBuf,
}

impl Tree {
    fn in_dir(shared: &Path) -> Self {
        let temp = tempfile::Builder::new()
            .prefix("ao-credentials-")
            .tempdir_in(shared)
            .unwrap();
        let base = temp.path().canonicalize().unwrap();
        chmod(&base, 0o700);
        let data = base.join("data");
        mkdir(&data, 0o700);
        Self {
            _temp: temp,
            base,
            data,
        }
    }

    /// A tree on the disk-backed shared temporary directory.
    fn new() -> Self {
        Self::in_dir(&org_platform::dirs::testing::disk_backed_temp().unwrap())
    }

    /// A tree for a test that needs the store to be accepted, or `None`, with the
    /// reason printed, where the disk-backed temporary directory is on a filesystem
    /// the store refuses: the filesystem decision then carries the coverage
    /// (`rules.rs`).
    fn accepted() -> Option<Self> {
        let tree = Self::new();
        let directory = File::open(&tree.base).unwrap();
        let kind = filesystem_kind(directory.as_fd()).unwrap();
        if let Err(rule) = rules::filesystem(kind) {
            println!(
                "skipped: {} cannot hold a release directory here: {rule}",
                tree.base.display()
            );
            return None;
        }
        Some(tree)
    }

    /// A directory `<base>/<name>` with `mode`.
    fn dir(&self, name: &str, mode: u32) -> PathBuf {
        let path = self.base.join(name);
        mkdir(&path, mode);
        path
    }

    fn data_root(&self) -> File {
        File::open(&self.data).unwrap()
    }

    /// Opens the release directory at `path`, below this tree's base, with the walk
    /// started at the base.
    fn open_with(
        &self,
        path: &Path,
        data_root: &File,
        mount_point_required: bool,
        expect_items: bool,
    ) -> Result<Opened, CredentialError> {
        release_directory_below(
            &self.base,
            &ReleaseDirectory {
                path,
                data_root: data_root.as_fd(),
                mount_point_required,
                expect_items,
            },
            &STORES,
        )
        .map(|root| Opened { root })
    }

    /// Opens the release directory at `path` for a host with the item stores `stores`.
    fn open_for(
        &self,
        path: &Path,
        stores: &[ItemStore<'_>],
        expect_items: bool,
    ) -> Result<Opened, CredentialError> {
        release_directory_below(
            &self.base,
            &ReleaseDirectory {
                path,
                data_root: self.data_root().as_fd(),
                mount_point_required: false,
                expect_items,
            },
            stores,
        )
        .map(|root| Opened { root })
    }

    fn open(&self, path: &Path) -> Result<Opened, CredentialError> {
        self.open_with(path, &self.data_root(), false, false)
    }

    fn open_expecting_items(&self, path: &Path) -> Result<Opened, CredentialError> {
        self.open_with(path, &self.data_root(), false, true)
    }
}

/// Opens `path` from `/`, through the contract, with `data_root` as the data root.
fn from_filesystem_root(
    path: &Path,
    data_root: &File,
    expect_items: bool,
) -> Result<Opened, CredentialError> {
    contract(
        ReleaseDirectory {
            path,
            data_root: data_root.as_fd(),
            mount_point_required: false,
            expect_items,
        },
        &STORES,
    )
}

/// A directory for a test from `/` that needs the store accepted: a fresh directory in
/// cargo's test directory, `target/tmp` (the one integration tests get as
/// `CARGO_TARGET_TMPDIR`). `None`, with the reason printed, where that directory
/// cannot be made or used.
fn cargo_test_directory() -> Option<tempfile::TempDir> {
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    // <target>/<profile>/deps/<test binary>
    let target = executable.ancestors().nth(3)?;
    let tmp = target.join("tmp");
    match fs::create_dir(&tmp) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            println!("skipped: {} cannot be made: {error}", tmp.display());
            return None;
        }
    }
    match tempfile::Builder::new()
        .prefix("ao-credentials-")
        .tempdir_in(&tmp)
    {
        Ok(dir) => Some(dir),
        Err(error) => {
            println!("skipped: nothing can be made in {}: {error}", tmp.display());
            None
        }
    }
}

// ---------------------------------------------------------------------------------
// From `/`, through the contract.
// ---------------------------------------------------------------------------------

/// The early production path: the store is opened exactly as a host opens it, from
/// `/`, on this machine's real filesystem; a key is added, every handle is dropped,
/// and a second handle loads it.
#[test]
fn the_store_opens_from_the_filesystem_root_and_a_key_outlives_its_handles() {
    let Some(temp) = cargo_test_directory() else {
        return;
    };
    let base = temp.path().canonicalize().unwrap();
    chmod(&base, 0o755);
    let (store, data) = (base.join("credentials"), base.join("data"));
    mkdir(&store, 0o700);
    mkdir(&data, 0o700);
    let data_root = File::open(&data).unwrap();

    let opened = match from_filesystem_root(&store, &data_root, false) {
        Ok(opened) => opened,
        Err(CredentialError::Root(refused))
            if base.starts_with(&refused.path) && base != refused.path =>
        {
            // A directory above cargo's test directory breaks a rule: this machine's
            // layout, not the store's. The tests from a made directory run the same
            // checks below it.
            println!("skipped: no directory here passes the ancestor rule: {refused}");
            return;
        }
        Err(CredentialError::Root(refused))
            if matches!(refused.rule, RootRule::Filesystem { .. }) =>
        {
            println!("skipped: {refused}");
            return;
        }
        Err(other) => panic!("the release directory was refused: {other}"),
    };
    println!(
        "opened from /: {} (filesystem type {:#x})",
        store.display(),
        filesystem_kind(File::open(&store).unwrap().as_fd()).unwrap()
    );
    let keys = opened.store(OFFICE_KEYS).unwrap();
    let key = [0x5a_u8; 32];
    keys.add(&name("0197c9f0-7c1e-7b7a-9f5e-3b1d2c4a5e6f"), &key)
        .unwrap();
    drop(keys);
    drop(opened);

    // Records now say a key exists: the reopened store must hold it.
    let reopened = from_filesystem_root(&store, &data_root, true).unwrap();
    let keys = reopened.store(OFFICE_KEYS).unwrap();
    assert_eq!(
        loaded(&keys, "0197c9f0-7c1e-7b7a-9f5e-3b1d2c4a5e6f").unwrap(),
        key
    );
    // LNX-4: owner-only, the service user's, and nothing in the data root.
    for (path, expected) in [
        (store.clone(), 0o700),
        (store.join("office-keys"), 0o700),
        (store.join(".lock"), 0o600),
        (
            store.join("office-keys/0197c9f0-7c1e-7b7a-9f5e-3b1d2c4a5e6f.key"),
            0o600,
        ),
    ] {
        let found = fs::symlink_metadata(&path).unwrap();
        assert_eq!(found.mode() & 0o7777, expected, "{}", path.display());
        assert_eq!(found.uid(), euid(), "{}", path.display());
    }
    assert!(listing(&data).is_empty());
}

#[test]
fn a_path_that_is_relative_or_goes_up_is_refused_before_anything_is_opened() {
    let data_root = File::open("/").unwrap();
    for (path, rule) in [
        ("", RootRule::Relative),
        ("secrets", RootRule::Relative),
        ("./secrets", RootRule::Relative),
        ("../secrets", RootRule::Relative),
        ("/secrets/..", RootRule::ParentComponent),
        (
            "/no-such-directory-of-this-test/../x",
            RootRule::ParentComponent,
        ),
    ] {
        assert_eq!(
            refusal(from_filesystem_root(Path::new(path), &data_root, false)),
            (rule, PathBuf::from(path)),
            "{path:?}"
        );
    }
    let with_nul = Path::new(std::ffi::OsStr::from_bytes(b"/a\0b"));
    assert_eq!(
        refusal(from_filesystem_root(with_nul, &data_root, false)),
        (RootRule::NotAPath, with_nul.to_path_buf())
    );
}

/// Shared temporary directories are writable by others, so nothing below one is ever
/// a release directory: the walk from `/` stops at the shared directory and names it,
/// before anything below it is opened.
#[test]
fn a_store_below_a_directory_writable_by_others_is_refused_from_the_filesystem_root() {
    let data = tempfile::tempdir().unwrap();
    let data_root = File::open(data.path()).unwrap();
    for shared in ["/tmp", "/var/tmp", "/dev/shm"] {
        let shared = Path::new(shared);
        let Ok(found) = fs::symlink_metadata(shared) else {
            println!("skipped: no {}", shared.display());
            continue;
        };
        if !found.is_dir() || found.mode() & 0o002 == 0 {
            println!(
                "skipped: {} is not a directory writable by others here",
                shared.display()
            );
            continue;
        }
        let temp = tempfile::tempdir_in(shared).unwrap();
        let store = temp.path().join("credentials");
        mkdir(&store, 0o700);
        // The shared directory itself must be reachable without a link for the walk
        // to get to it (`/tmp` is a link on some systems).
        let resolved = store.canonicalize().unwrap();
        let (rule, path) = refusal(from_filesystem_root(&resolved, &data_root, false));
        assert_eq!(
            rule,
            RootRule::AncestorWritableByOthers,
            "{}",
            path.display()
        );
        assert!(
            resolved.starts_with(&path) && path != resolved,
            "{} is not above {}",
            path.display(),
            resolved.display()
        );
        assert_ne!(fs::symlink_metadata(&path).unwrap().mode() & 0o002, 0);
        // Nothing was made in the store.
        assert!(listing(&store).is_empty());
    }
}

#[test]
fn a_missing_component_and_one_that_is_no_directory_are_named_from_the_filesystem_root() {
    let data_root = File::open("/").unwrap();
    let missing = Path::new("/no-such-directory-of-this-test/credentials");
    assert_eq!(
        refusal(from_filesystem_root(missing, &data_root, false)),
        (
            RootRule::Unavailable(ErrorKind::NotFound),
            PathBuf::from("/no-such-directory-of-this-test")
        )
    );
    // `/proc/self` is a symbolic link on every Linux: never followed.
    assert_eq!(
        refusal(from_filesystem_root(
            Path::new("/proc/self/cwd"),
            &data_root,
            false
        )),
        (RootRule::NotADirectory, PathBuf::from("/proc/self"))
    );
    // A regular file on the way.
    assert_eq!(
        refusal(from_filesystem_root(
            Path::new("/proc/version/x"),
            &data_root,
            false
        )),
        (RootRule::NotADirectory, PathBuf::from("/proc/version"))
    );
}

/// A directory of another user, on the way or as the store, from `/`. As root the
/// test makes one; as another user it uses what the machine has (`/home` and the
/// directories in it), and says so when it has nothing.
#[test]
fn a_directory_of_another_user_is_refused_as_an_ancestor_and_as_the_store() {
    let data = tempfile::tempdir().unwrap();
    let data_root = File::open(data.path()).unwrap();
    if is_root() {
        let Some(tree) = Tree::accepted() else { return };
        let foreign = tree.dir("foreign", 0o755);
        mkdir(&foreign.join("credentials"), 0o700);
        std::os::unix::fs::chown(&foreign, Some(65534), Some(65534)).unwrap();
        assert_eq!(
            refusal(tree.open(&foreign.join("credentials"))),
            (RootRule::AncestorOwner, foreign.clone())
        );
        let store = tree.dir("credentials", 0o700);
        std::os::unix::fs::chown(&store, Some(65534), Some(65534)).unwrap();
        assert_eq!(
            refusal(tree.open(&store)),
            (RootRule::StoreOwner, store.clone())
        );
        assert_eq!(mode(&store), 0o700);
        assert_eq!(fs::symlink_metadata(&store).unwrap().uid(), 65534);
        assert!(listing(&store).is_empty());
        return;
    }
    // Not root. `/home` is root's: as the store it has the wrong owner.
    let home = Path::new("/home");
    match fs::symlink_metadata(home) {
        Ok(found) if found.is_dir() && found.uid() == 0 && found.mode() & 0o022 == 0 => {
            assert_eq!(
                refusal(from_filesystem_root(home, &data_root, false)),
                (RootRule::StoreOwner, home.to_path_buf())
            );
        }
        _ => println!("skipped: /home is not a closed directory of root here"),
    }
    // A directory in `/home` of a user that is neither root nor this one: the walk
    // stops at it, before it looks for anything inside.
    let foreign = fs::read_dir(home).ok().and_then(|entries| {
        entries.flatten().map(|entry| entry.path()).find(|path| {
            fs::symlink_metadata(path)
                .is_ok_and(|found| found.is_dir() && found.uid() != 0 && found.uid() != euid())
        })
    });
    match foreign {
        Some(foreign) => assert_eq!(
            refusal(from_filesystem_root(
                &foreign.join("credentials"),
                &data_root,
                false
            )),
            (RootRule::AncestorOwner, foreign)
        ),
        None => println!(
            "skipped: no directory of another user in /home, and only root can make one; \
             the ancestor decision covers the rule"
        ),
    }
}

// ---------------------------------------------------------------------------------
// From a directory the test made: every rule on a real tree.
// ---------------------------------------------------------------------------------

#[test]
fn a_checked_store_keeps_items_across_handles_and_is_owner_only() {
    let Some(tree) = Tree::accepted() else { return };
    let parent = tree.dir("state", 0o755);
    let store = parent.join("credentials");
    mkdir(&store, 0o700);

    let opened = tree.open(&store).unwrap();
    let keys = opened.store(OFFICE_KEYS).unwrap();
    let secrets = opened.store(HOST_SECRETS).unwrap();
    keys.add(&name("office-a"), b"key bytes").unwrap();
    secrets.replace(&name("registry"), b"registry key").unwrap();
    assert_eq!(
        keys.add(&name("office-a"), b"other").unwrap_err().kind,
        CredentialError::Exists
    );
    drop((keys, secrets, opened));

    let reopened = tree.open_expecting_items(&store).unwrap();
    let keys = reopened.store(OFFICE_KEYS).unwrap();
    let secrets = reopened.store(HOST_SECRETS).unwrap();
    assert_eq!(loaded(&keys, "office-a").unwrap(), b"key bytes");
    assert_eq!(loaded(&secrets, "registry").unwrap(), b"registry key");
    assert_eq!(
        listing(&store),
        [
            ".lock",
            "host-secrets",
            "host-secrets/.lock",
            "host-secrets/registry",
            "office-keys",
            "office-keys/.lock",
            "office-keys/office-a.key",
        ]
    );
    // LNX-4: only the service user can read any of it, and none of it is in the data
    // root.
    for entry in listing(&store) {
        let path = store.join(&entry);
        let found = fs::symlink_metadata(&path).unwrap();
        let expected = if found.is_dir() { 0o700 } else { 0o600 };
        assert_eq!(found.mode() & 0o7777, expected, "{entry}");
        assert_eq!(found.uid(), euid(), "{entry}");
        assert_eq!(found.nlink(), if found.is_dir() { 2 } else { 1 }, "{entry}");
    }
    assert_eq!(mode(&store), 0o700);
    assert!(listing(&tree.data).is_empty());
}

#[test]
fn the_walks_start_can_itself_be_the_store() {
    let Some(tree) = Tree::accepted() else { return };
    // The data root must then be elsewhere.
    let elsewhere = Tree::new();
    let opened = tree
        .open_with(&tree.base, &elsewhere.data_root(), false, false)
        .unwrap();
    opened
        .store(HOST_SECRETS)
        .unwrap()
        .replace(&name("registry"), b"x")
        .unwrap();
    // With its own data directory as the data root, it contains the data root.
    drop(opened);
    assert_eq!(
        refusal(tree.open(&tree.base)),
        (RootRule::ContainsDataRoot, tree.base.clone())
    );
}

#[test]
fn an_ancestor_writable_by_others_is_refused_and_named() {
    let tree = Tree::new();
    for open_mode in [0o777, 0o1777, 0o757, 0o703] {
        let outer = tree.base.join(format!("outer-{open_mode:o}"));
        mkdir(&outer, 0o755);
        let inner = outer.join("inner");
        mkdir(&inner, 0o755);
        let store = inner.join("credentials");
        mkdir(&store, 0o700);
        // The directory two above the store, then the one directly above it.
        chmod(&outer, open_mode);
        assert_eq!(
            refusal(tree.open(&store)),
            (RootRule::AncestorWritableByOthers, outer.clone()),
            "{open_mode:o}"
        );
        chmod(&outer, 0o755);
        chmod(&inner, open_mode);
        assert_eq!(
            refusal(tree.open(&store)),
            (RootRule::AncestorWritableByOthers, inner.clone()),
            "{open_mode:o}"
        );
        // Nothing was repaired, and nothing was made in the store.
        assert_eq!(mode(&inner), open_mode);
        assert!(listing(&store).is_empty());
    }
}

#[test]
fn an_ancestor_writable_by_its_group_passes_only_under_the_users_private_group() {
    let Some(tree) = Tree::accepted() else { return };
    let parent = tree.dir("parent", 0o775);
    let store = parent.join("credentials");
    mkdir(&store, 0o700);
    let group = fs::symlink_metadata(&parent).unwrap().gid();
    let private = account::private_group_of(euid()).unwrap();
    println!(
        "uid {}: private group {private:?}; the directory's group is {group}",
        euid()
    );
    // The directory was made with this process's group. It is admitted exactly when
    // that group is this user's private group, as on a default Ubuntu account.
    let opened = tree.open(&store);
    if private == Some(group) {
        drop(opened.unwrap());
        // Set-group-id on such a directory changes nothing.
        chmod(&parent, 0o2775);
        drop(tree.open(&store).unwrap());
    } else {
        assert_eq!(
            refusal(opened),
            (RootRule::AncestorWritableByGroup, parent.clone())
        );
    }
    // Any other group: refused, sticky or not (the sticky exception is root's own
    // directories').
    let Some(other) = another_group() else {
        println!(
            "skipped: this user has no second group to give a directory; the ancestor \
             decision covers the refusal"
        );
        return;
    };
    chgrp(&parent, other);
    let sticky_passes = is_root();
    for (group_mode, passes) in [(0o775, false), (0o770, false), (0o1775, sticky_passes)] {
        chmod(&parent, group_mode);
        let opened = tree.open(&store);
        if passes {
            // As root the directory is root's own, and sticky: the first exception.
            drop(opened.unwrap());
        } else {
            assert_eq!(
                refusal(opened),
                (RootRule::AncestorWritableByGroup, parent.clone()),
                "{group_mode:o}"
            );
        }
        assert_eq!(mode(&parent), group_mode);
    }
    // Without the group's write bit the group does not matter.
    chmod(&parent, 0o755);
    drop(tree.open(&store).unwrap());
}

#[test]
fn the_store_directory_must_have_mode_0700_exactly_and_nothing_repairs_it() {
    let tree = Tree::new();
    let store = tree.dir("credentials", 0o700);
    for wrong in [0o750, 0o600, 0o777, 0o755, 0o711, 0o1700, 0o2700] {
        chmod(&store, wrong);
        assert_eq!(
            refusal(tree.open(&store)),
            (RootRule::StoreMode, store.clone()),
            "{wrong:o}"
        );
        assert_eq!(mode(&store), wrong, "the mode was changed");
    }
    chmod(&store, 0o700);
    assert!(
        listing(&store).is_empty(),
        "something was made in a refused store"
    );
    // A store this user cannot even read is unavailable, and named.
    if !is_root() {
        chmod(&store, 0o000);
        assert_eq!(
            refusal(tree.open(&store)),
            (
                RootRule::Unavailable(ErrorKind::PermissionDenied),
                store.clone()
            )
        );
        chmod(&store, 0o700);
    }
}

#[test]
fn a_symbolic_link_in_any_component_is_refused_and_its_target_is_left_alone() {
    let tree = Tree::new();
    let real_parent = tree.dir("real", 0o755);
    let real_store = real_parent.join("credentials");
    mkdir(&real_store, 0o700);

    // The store itself is a link to a perfectly good store.
    let linked_store = tree.base.join("linked-store");
    symlink(&real_store, &linked_store).unwrap();
    assert_eq!(
        refusal(tree.open(&linked_store)),
        (RootRule::NotADirectory, linked_store.clone())
    );
    // A directory above it is a link.
    let linked_parent = tree.base.join("linked-parent");
    symlink(&real_parent, &linked_parent).unwrap();
    assert_eq!(
        refusal(tree.open(&linked_parent.join("credentials"))),
        (RootRule::NotADirectory, linked_parent.clone())
    );
    // A link two levels up, and a dangling one.
    let deep = tree.dir("deep", 0o755);
    symlink(&tree.base, deep.join("up")).unwrap();
    assert_eq!(
        refusal(tree.open(&deep.join("up/real/credentials"))),
        (RootRule::NotADirectory, deep.join("up"))
    );
    symlink(tree.base.join("nowhere"), tree.base.join("dangling")).unwrap();
    assert_eq!(
        refusal(tree.open(&tree.base.join("dangling"))),
        (RootRule::NotADirectory, tree.base.join("dangling"))
    );
    // A regular file and a missing directory, as the store and on the way.
    fs::write(tree.base.join("file"), b"").unwrap();
    for (path, rule, named) in [
        ("file", RootRule::NotADirectory, "file"),
        ("file/credentials", RootRule::NotADirectory, "file"),
        (
            "missing",
            RootRule::Unavailable(ErrorKind::NotFound),
            "missing",
        ),
        (
            "missing/credentials",
            RootRule::Unavailable(ErrorKind::NotFound),
            "missing",
        ),
        (
            "real/missing",
            RootRule::Unavailable(ErrorKind::NotFound),
            "real/missing",
        ),
    ] {
        assert_eq!(
            refusal(tree.open(&tree.base.join(path))),
            (rule, tree.base.join(named)),
            "{path}"
        );
    }
    // Nothing was made through a link, and nothing is ever created on the way.
    assert!(listing(&real_store).is_empty());
    assert!(!tree.base.join("missing").exists());
}

#[test]
fn the_store_and_the_data_root_must_be_disjoint_however_each_is_reached() {
    let Some(tree) = Tree::accepted() else { return };
    // The store inside the data root, directly and deeper.
    let inside = tree.data.join("credentials");
    mkdir(&inside, 0o700);
    assert_eq!(
        refusal(tree.open(&inside)),
        (RootRule::InsideDataRoot, inside.clone())
    );
    mkdir(&tree.data.join("deep"), 0o755);
    let deeper = tree.data.join("deep/credentials");
    mkdir(&deeper, 0o700);
    assert_eq!(
        refusal(tree.open(&deeper)),
        (RootRule::InsideDataRoot, deeper.clone())
    );
    // The data root inside the store, directly and deeper.
    let store = tree.dir("credentials", 0o700);
    mkdir(&store.join("data"), 0o700);
    mkdir(&store.join("a"), 0o700);
    mkdir(&store.join("a/data"), 0o700);
    for nested in ["data", "a/data"] {
        let data_root = File::open(store.join(nested)).unwrap();
        assert_eq!(
            refusal(tree.open_with(&store, &data_root, false, false)),
            (RootRule::ContainsDataRoot, store.clone()),
            "{nested}"
        );
    }
    // The same directory as both.
    let same = File::open(&store).unwrap();
    assert_eq!(
        refusal(tree.open_with(&store, &same, false, false)),
        (RootRule::IsDataRoot, store.clone())
    );
    // The same directory through two paths: the data root was opened through a link,
    // and through a path that goes down and up again. Paths do not decide; the
    // directory's device and inode do.
    let alias = tree.base.join("alias");
    symlink(&store, &alias).unwrap();
    for other_path in [alias.clone(), store.join("a/..")] {
        let through = File::open(&other_path).unwrap();
        assert_eq!(
            refusal(tree.open_with(&store, &through, false, false)),
            (RootRule::IsDataRoot, store.clone()),
            "{}",
            other_path.display()
        );
    }
    // The store inside a data root that was opened through a link.
    let data_alias = tree.base.join("data-alias");
    symlink(&tree.data, &data_alias).unwrap();
    let through = File::open(&data_alias).unwrap();
    assert_eq!(
        refusal(tree.open_with(&inside, &through, false, false)),
        (RootRule::InsideDataRoot, inside.clone())
    );
    // A data root that is no directory proves nothing.
    fs::write(tree.base.join("file"), b"").unwrap();
    let file = File::open(tree.base.join("file")).unwrap();
    assert_eq!(
        refusal(tree.open_with(&store, &file, false, false)),
        (RootRule::DataRoot(ErrorKind::NotADirectory), store.clone())
    );
    // No refused open made anything, and a sibling data root is accepted.
    assert_eq!(listing(&store), ["a", "a/data", "data"]);
    assert!(listing(&inside).is_empty());
    drop(tree.open(&store).unwrap());
}

#[test]
fn the_walk_up_from_a_directory_ends_at_the_filesystem_root() {
    let tree = Tree::new();
    let nested = tree.base.join("a");
    mkdir(&nested, 0o700);
    let found = chain(File::open(&nested).unwrap().as_fd()).unwrap();
    // The directory itself first, then each directory above it by its own path.
    let expected: Vec<Identity> = nested
        .ancestors()
        .map(|path| {
            let found = fs::metadata(path).unwrap();
            (found.dev(), found.ino())
        })
        .collect();
    assert_eq!(found, expected);
    assert_eq!(
        found.last().copied(),
        fs::metadata("/").ok().map(|root| (root.dev(), root.ino()))
    );
    assert_eq!(chain(File::open("/").unwrap().as_fd()).unwrap().len(), 1);
    fs::write(tree.base.join("file"), b"").unwrap();
    let file = File::open(tree.base.join("file")).unwrap();
    assert_eq!(
        chain(file.as_fd()).unwrap_err().kind(),
        ErrorKind::NotADirectory
    );
}

/// What this machine's filesystems really are, and that a memory filesystem is
/// refused as the store. The report is the record of what the run tested.
#[test]
fn a_memory_filesystem_is_refused_and_the_disk_filesystem_is_reported() {
    let disk = org_platform::dirs::testing::disk_backed_temp().unwrap();
    let kind = filesystem_kind(File::open(&disk).unwrap().as_fd()).unwrap();
    println!(
        "{} is on filesystem type {kind:#x}: {}",
        disk.display(),
        match rules::filesystem(kind) {
            Ok(()) => "accepted".to_owned(),
            Err(rule) => format!("refused ({rule})"),
        }
    );
    // `/proc` is neither a disk nor one of the three named classes.
    let proc_kind = filesystem_kind(File::open("/proc").unwrap().as_fd()).unwrap();
    assert_eq!(
        rules::filesystem(proc_kind),
        Err(RootRule::Filesystem {
            class: FilesystemClass::Other,
            kind: 0x9fa0
        })
    );

    let Some(memory) = org_platform::dirs::testing::memory_backed_temp().unwrap() else {
        println!("skipped: this system mounts no memory filesystem by default");
        return;
    };
    let memory_kind = match File::open(&memory) {
        Ok(directory) => filesystem_kind(directory.as_fd()).unwrap(),
        Err(error) => {
            println!("skipped: {} cannot be opened: {error}", memory.display());
            return;
        }
    };
    if !matches!(
        rules::filesystem(memory_kind),
        Err(RootRule::Filesystem {
            class: FilesystemClass::Memory,
            ..
        })
    ) {
        println!(
            "skipped: {} is on filesystem type {memory_kind:#x}, not a memory filesystem",
            memory.display()
        );
        return;
    }
    let tree = Tree::in_dir(&memory);
    let store = tree.dir("credentials", 0o700);
    let (rule, path) = refusal(tree.open(&store));
    assert_eq!(
        rule,
        RootRule::Filesystem {
            class: FilesystemClass::Memory,
            kind: memory_kind
        }
    );
    assert_eq!(path, store);
    let text = rule.to_string();
    for part in [
        "network",
        "user-space",
        "memory",
        "ext4",
        "XFS",
        "Btrfs",
        "F2FS",
    ] {
        assert!(text.contains(part), "{text}");
    }
    // Refused with records or without, and nothing was made.
    assert_eq!(refusal(tree.open_expecting_items(&store)).0, rule);
    assert!(listing(&store).is_empty());
    println!(
        "refused as the store: {} (filesystem type {memory_kind:#x})",
        store.display()
    );
}

#[test]
fn a_required_mount_point_refuses_an_ordinary_directory_and_the_probe_knows_mount_roots() {
    // The live probe: `/` and `/proc` are the roots of mounts, a directory this test
    // made is not, and this kernel reports the attribute.
    for mounted in ["/", "/proc"] {
        let (attributes, reported) =
            mount_attributes(File::open(mounted).unwrap().as_fd()).unwrap();
        assert_eq!(
            rules::mount_point(attributes, reported),
            Ok(()),
            "{mounted}"
        );
    }
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let (attributes, reported) = mount_attributes(File::open(&store).unwrap().as_fd()).unwrap();
    assert_eq!(
        rules::mount_point(attributes, reported),
        Err(RootRule::NotAMountPoint)
    );
    // Through the open: required, an ordinary directory refuses; not required, it
    // opens.
    assert_eq!(
        refusal(tree.open_with(&store, &tree.data_root(), true, false)),
        (RootRule::NotAMountPoint, store.clone())
    );
    assert!(listing(&store).is_empty());
    drop(
        tree.open_with(&store, &tree.data_root(), false, false)
            .unwrap(),
    );
}

/// The other way of the mount-point rule: a store that is the root of a mount is
/// accepted when one is required, next to a data root on a volume of its own (where a
/// mount point is required the store may not be in the data root's volume, and a data
/// root on the machine's own disk has all of that disk as its volume). Making a mount
/// takes root (and a container that allows it); otherwise the reason is printed and
/// the probe test above, with the decision's tests, carries the rule.
#[test]
fn a_required_mount_point_accepts_a_store_that_is_the_root_of_a_mount() {
    if !is_root() {
        println!(
            "skipped: only root can mount, and no mount root here is a directory this user \
             owns with mode 0700"
        );
        return;
    }
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let target = CString::new(store.as_os_str().as_bytes()).unwrap();
    // SAFETY: NUL-terminated paths that outlive the call; a bind mount of the
    // directory onto itself reads no filesystem type and no data.
    let mounted = unsafe {
        libc::mount(
            target.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    if mounted != 0 {
        println!(
            "skipped: this environment refuses a bind mount: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    // The data volume: a directory of its own, mounted where the data root is.
    let data_volume = tree.dir("data-volume", 0o700);
    let _data_mount = match bind(&data_volume, &tree.data) {
        Ok(mount) => mount,
        Err(error) => {
            // SAFETY: a NUL-terminated path; `MNT_DETACH` cannot block.
            unsafe { libc::umount2(target.as_ptr(), libc::MNT_DETACH) };
            println!("skipped: this environment refuses a second bind mount: {error}");
            return;
        }
    };
    // An empty mounted volume with records: credentials missing, as after a replaced
    // container with a new volume.
    let first = tree
        .open_with(&store, &tree.data_root(), true, true)
        .map(|_| ())
        .map_err(|error| error.to_string());
    let second = tree
        .open_with(&store, &tree.data_root(), true, false)
        .map(|root| {
            root.store(HOST_SECRETS)
                .unwrap()
                .replace(&name("registry"), b"x")
                .unwrap();
        })
        .map_err(|error| error.to_string());
    let third = tree
        .open_with(&store, &tree.data_root(), true, true)
        .map(|_| ())
        .map_err(|error| error.to_string());
    // Unmount before any assertion, so that a failure leaves no mount behind.
    // SAFETY: a NUL-terminated path; `MNT_DETACH` cannot block on a busy mount.
    let unmounted = unsafe { libc::umount2(target.as_ptr(), libc::MNT_DETACH) };
    assert_eq!(unmounted, 0);
    assert!(
        first.is_err_and(|text| text.contains("credentials missing")),
        "an empty mounted store with records was not refused as missing"
    );
    assert_eq!(second, Ok(()));
    assert_eq!(third, Ok(()));
    println!(
        "accepted as a mount root: a bind mount at {}",
        store.display()
    );
}

#[test]
fn a_store_found_empty_when_records_say_keys_exist_is_refused_and_nothing_is_made() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let missing = (RootRule::CredentialsMissing, store.clone());

    // A fresh, empty directory: the lost volume. Not even the lock is made.
    assert_eq!(refusal(tree.open_expecting_items(&store)), missing);
    assert!(listing(&store).is_empty());
    let text = RootRule::CredentialsMissing.to_string();
    assert!(text.starts_with("credentials missing"), "{text}");

    // Without records the same directory opens, and both views can be made.
    let opened = tree.open(&store).unwrap();
    let keys = opened.store(OFFICE_KEYS).unwrap();
    let secrets = opened.store(HOST_SECRETS).unwrap();
    drop((keys, secrets, opened));
    // Both views exist and both are empty: still missing. Locks are not items.
    assert_eq!(
        listing(&store),
        [
            ".lock",
            "host-secrets",
            "host-secrets/.lock",
            "office-keys",
            "office-keys/.lock"
        ]
    );
    assert_eq!(refusal(tree.open_expecting_items(&store)), missing);

    // What is not an item does not make the store hold one. In an item store: a
    // temporary file, a file without the store's suffix, a file whose name the host's
    // own rule refuses, a name that is no item name. Outside the item stores: a file,
    // a directory that looks like a store, and a volume's `lost+found`, which this
    // user cannot even read. None is opened, and none is counted.
    let plain = |path: PathBuf| {
        fs::write(&path, b"not an item").unwrap();
        chmod(&path, 0o600);
    };
    plain(store.join("office-keys/.put.0123456789abcdef"));
    plain(store.join("office-keys/README"));
    plain(store.join("office-keys/office-a.key.bak"));
    plain(store.join("office-keys/not-an-office.key"));
    plain(store.join("office-keys/a b.key"));
    plain(store.join("host-secrets/README"));
    plain(store.join("host-secrets/.hidden"));
    mkdir(&store.join("office-keys/a-directory"), 0o700);
    plain(store.join("stray-file"));
    plain(store.join("office-a.key"));
    mkdir(&store.join("backup"), 0o700);
    plain(store.join("backup/office-a.key"));
    plain(store.join("backup/registry"));
    mkdir(&store.join("lost+found"), 0o700);
    plain(store.join("lost+found/office-a.key"));
    chmod(&store.join("lost+found"), 0o000);
    symlink(&tree.data, store.join("a-link")).unwrap();
    assert_eq!(refusal(tree.open_expecting_items(&store)), missing);
    // Without records the same directory opens with all of that in it.
    drop(tree.open(&store).unwrap());
    chmod(&store.join("lost+found"), 0o700);

    // One view empty and the other not, both ways: the store holds an item.
    for (view, file) in [
        ("office-keys", "office-a.key"),
        ("host-secrets", "registry"),
    ] {
        let item = store.join(view).join(file);
        fs::write(&item, b"an item").unwrap();
        chmod(&item, 0o600);
        drop(tree.open_expecting_items(&store).unwrap());
        fs::remove_file(&item).unwrap();
        assert_eq!(
            refusal(tree.open_expecting_items(&store)),
            missing,
            "{view}"
        );
    }
}

#[test]
fn a_second_host_on_the_same_store_is_refused_until_the_first_has_dropped_every_handle() {
    const MARKER: &str = "ORG_RELEASE_DIRECTORY_LOCK_CHILD";
    const TEST: &str = "credentials::linux::release::tests::\
         a_second_host_on_the_same_store_is_refused_until_the_first_has_dropped_every_handle";
    if let Some(base) = std::env::var_os(MARKER) {
        // Another process: the second host. It makes the same walk to the same store.
        let base = PathBuf::from(base);
        let (store, data) = (base.join("credentials"), base.join("data"));
        let data_root = File::open(&data).unwrap();
        let second = release_directory_below(
            &base,
            &ReleaseDirectory {
                path: &store,
                data_root: data_root.as_fd(),
                mount_point_required: false,
                expect_items: false,
            },
            &STORES,
        )
        .map(|root| Opened { root });
        assert_eq!(refusal(second), (RootRule::InUse, store));
        return;
    }
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let in_use = (RootRule::InUse, store.clone());

    let first = tree.open(&store).unwrap();
    assert_eq!(refusal(tree.open(&store)), in_use);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env(MARKER, &tree.base)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    // A view keeps the store's lock when the root handle is gone.
    let keys = first.store(OFFICE_KEYS).unwrap();
    drop(first);
    assert_eq!(refusal(tree.open(&store)), in_use);
    keys.add(&name("office-a"), b"key").unwrap();
    drop(keys);
    // Every handle dropped: the next host opens it.
    let next = tree.open_expecting_items(&store).unwrap();
    assert_eq!(
        loaded(&next.store(OFFICE_KEYS).unwrap(), "office-a").unwrap(),
        b"key"
    );
}

#[test]
fn a_lock_that_is_not_an_owner_only_regular_file_is_refused_and_left_as_it_is() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let lock = store.join(".lock");
    let refused = (RootRule::LockFile, store.clone());

    // Another mode.
    fs::write(&lock, b"").unwrap();
    for wrong in [0o644, 0o660, 0o400, 0o700] {
        chmod(&lock, wrong);
        // One this user cannot open for the lock at all (`0400`) is refused by the
        // lock's rule as well, not as a part of the path that could not be opened.
        assert_eq!(refusal(tree.open(&store)), refused, "{wrong:o}");
        assert_eq!(mode(&lock), wrong, "the lock's mode was changed");
    }
    // A second link.
    chmod(&lock, 0o600);
    fs::hard_link(&lock, store.join("second-name")).unwrap();
    assert_eq!(refusal(tree.open(&store)), refused);
    fs::remove_file(store.join("second-name")).unwrap();
    drop(tree.open(&store).unwrap());
    // A link, a directory and a FIFO at the lock's name.
    fs::remove_file(&lock).unwrap();
    let victim = tree.base.join("victim");
    fs::write(&victim, b"keep").unwrap();
    chmod(&victim, 0o600);
    symlink(&victim, &lock).unwrap();
    assert_eq!(refusal(tree.open(&store)), refused);
    assert_eq!(fs::read(&victim).unwrap(), b"keep");
    fs::remove_file(&lock).unwrap();
    mkdir(&lock, 0o700);
    assert_eq!(refusal(tree.open(&store)), refused);
    fs::remove_dir(&lock).unwrap();
    let fifo = CString::new(lock.as_os_str().as_bytes()).unwrap();
    // SAFETY: a NUL-terminated path in this test's own directory.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert_eq!(refusal(tree.open(&store)), refused);
    fs::remove_file(&lock).unwrap();
    // A file of another user: only root can make one.
    if is_root() {
        fs::write(&lock, b"").unwrap();
        chmod(&lock, 0o600);
        std::os::unix::fs::chown(&lock, Some(65534), Some(65534)).unwrap();
        assert_eq!(refusal(tree.open(&store)), refused);
        fs::remove_file(&lock).unwrap();
    }
    drop(tree.open(&store).unwrap());
    // The texts are the lock's own.
    let text = RootRule::LockFile.to_string();
    assert!(text.contains("lock file (.lock)"), "{text}");
    let text = RootRule::LockUnavailable(ErrorKind::ReadOnlyFilesystem).to_string();
    assert!(
        text.contains("lock file (.lock) could not be opened"),
        "{text}"
    );
}

/// A process whose file-creation mask takes the owner's own bits away (`0277`) makes
/// every file `0400` and every directory `0500`. What an open makes gets its mode set
/// explicitly, so such a host works; what already exists with such a mode was not made
/// by this open, and is refused and never repaired.
#[test]
fn a_lock_made_under_a_mask_that_takes_the_owners_bits_is_usable_and_an_old_one_is_not_repaired() {
    const MARKER: &str = "ORG_RELEASE_DIRECTORY_UMASK_CHILD";
    const TEST: &str = "credentials::linux::release::tests::\
         a_lock_made_under_a_mask_that_takes_the_owners_bits_is_usable_and_an_old_one_is_not_repaired";
    if let Some(base) = std::env::var_os(MARKER) {
        // A process of its own: the mask belongs to the whole process, and this
        // test's other threads make files too.
        let base = PathBuf::from(base);
        let (store, data) = (base.join("credentials"), base.join("data"));
        let data_root = File::open(&data).unwrap();
        let location = ReleaseDirectory {
            path: &store,
            data_root: data_root.as_fd(),
            mount_point_required: false,
            expect_items: false,
        };
        // SAFETY: this exact-test child runs nothing else.
        unsafe { libc::umask(0o277) };
        // Everything this open makes gets its mode set, whatever the mask is: the
        // root's lock, the item stores' directories and their locks, and an item.
        let opened = release_directory_below(&base, &location, &STORES)
            .map(|root| Opened { root })
            .unwrap();
        let keys = opened.store(OFFICE_KEYS).unwrap();
        keys.add(&name("office-a"), b"key").unwrap();
        drop((keys, opened));
        // The next host, under the same mask, finds every one of them as it must be.
        let again = release_directory_below(
            &base,
            &ReleaseDirectory {
                expect_items: true,
                ..location
            },
            &STORES,
        )
        .map(|root| Opened { root })
        .unwrap();
        assert_eq!(
            loaded(&again.store(OFFICE_KEYS).unwrap(), "office-a").unwrap(),
            b"key"
        );
        return;
    }
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env(MARKER, &tree.base)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    for (path, expected) in [
        (".lock", 0o600),
        ("office-keys", 0o700),
        ("office-keys/.lock", 0o600),
        ("office-keys/office-a.key", 0o600),
    ] {
        assert_eq!(mode(&store.join(path)), expected, "{path}");
    }
    // A lock that exists with such a mode was not made by this open: refused by the
    // lock's rule, and left exactly as it is.
    let lock = store.join(".lock");
    chmod(&lock, 0o400);
    assert_eq!(
        refusal(tree.open(&store)),
        (RootRule::LockFile, store.clone())
    );
    assert_eq!(mode(&lock), 0o400);
    chmod(&lock, 0o600);
    drop(tree.open(&store).unwrap());
}

#[test]
fn an_item_that_breaks_a_rule_is_refused_by_the_engine_and_nothing_is_repaired() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let opened = tree.open(&store).unwrap();
    let keys = opened.store(OFFICE_KEYS).unwrap();
    let dir = store.join("office-keys");
    let write = |file: &str, bytes: &[u8], file_mode: u32| {
        let path = dir.join(file);
        fs::write(&path, bytes).unwrap();
        chmod(&path, file_mode);
        path
    };

    // A wrong mode.
    for wrong in [0o644, 0o640, 0o400, 0o700] {
        let path = write("wrong-mode.key", b"key", wrong);
        assert_eq!(
            keys.load(&name("wrong-mode")).unwrap_err(),
            CredentialError::Unsafe,
            "{wrong:o}"
        );
        assert_eq!(
            keys.add(&name("wrong-mode"), b"new").unwrap_err().kind,
            CredentialError::Unsafe
        );
        assert_eq!(
            keys.delete(&name("wrong-mode")),
            Err(CredentialError::Unsafe)
        );
        assert_eq!(mode(&path), wrong, "the mode was repaired");
        chmod(&path, 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"key");
        fs::remove_file(&path).unwrap();
    }
    // A second link.
    let linked = write("linked.key", b"key", 0o600);
    fs::hard_link(&linked, tree.base.join("second-name")).unwrap();
    assert_eq!(
        keys.load(&name("linked")).unwrap_err(),
        CredentialError::Unsafe
    );
    assert_eq!(fs::symlink_metadata(&linked).unwrap().nlink(), 2);
    fs::remove_file(tree.base.join("second-name")).unwrap();
    assert_eq!(loaded(&keys, "linked").unwrap(), b"key");
    // Too large.
    let large = write("large.key", &vec![7u8; MAX_ITEM_BYTES + 1], 0o600);
    assert_eq!(
        keys.load(&name("large")).unwrap_err(),
        CredentialError::Corrupt
    );
    assert_eq!(
        fs::symlink_metadata(&large).unwrap().len(),
        MAX_ITEM_BYTES as u64 + 1
    );
    assert_eq!(
        keys.add(&name("too-large"), &vec![7u8; MAX_ITEM_BYTES + 1])
            .unwrap_err()
            .kind,
        CredentialError::TooLarge
    );
    // A link where an item would be.
    let victim = tree.base.join("victim");
    fs::write(&victim, b"keep").unwrap();
    chmod(&victim, 0o600);
    symlink(&victim, dir.join("link.key")).unwrap();
    assert!(keys.load(&name("link")).is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"keep");
    // Another owner: only root can make such a file.
    if is_root() {
        let foreign = write("foreign.key", b"key", 0o600);
        std::os::unix::fs::chown(&foreign, Some(65534), Some(65534)).unwrap();
        assert_eq!(
            keys.load(&name("foreign")).unwrap_err(),
            CredentialError::Unsafe
        );
        assert_eq!(fs::symlink_metadata(&foreign).unwrap().uid(), 65534);
    } else {
        println!(
            "skipped: an item of another user (only root can make one); the engine's one \
             check of owner, mode and links refused the wrong mode and the second link above"
        );
    }
    // No error of an item operation names a path.
    for error in [CredentialError::Unsafe, CredentialError::Corrupt] {
        assert!(!error.to_string().contains('/'), "{error}");
    }
}

#[test]
fn an_item_store_is_a_single_checked_directory_below_the_root() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let opened = tree.open(&store).unwrap();
    let items = |directory, suffix| ReleaseItems {
        directory,
        suffix,
        sharing: Sharing::Exclusive,
    };
    // Not a single plain name, a suffix that is none, a store the directory was not
    // opened for, or one of its stores under another suffix: refused, and nothing is
    // made.
    for (directory, suffix) in [
        ("", ""),
        (".", ""),
        ("..", ""),
        (".hidden", ""),
        ("a/b", ""),
        ("../outside", ""),
        ("a b", ""),
        (".lock", ""),
        ("keys", "/x"),
        ("backup", ""),
        ("office-keys", ""),
        ("office-keys", ".bak"),
        ("host-secrets", ".key"),
    ] {
        assert_eq!(
            opened.store(items(directory, suffix)).unwrap_err(),
            CredentialError::InvalidName,
            "{directory:?} {suffix:?}"
        );
    }
    assert_eq!(listing(&store), [".lock"]);
    assert!(!tree.base.join("outside").exists());
    // A declaration that is no item store's is refused before anything is opened.
    drop(opened);
    for bad in [
        ItemStore {
            directory: "a/b",
            ..STORES[0]
        },
        ItemStore {
            directory: ".lock",
            ..STORES[0]
        },
        ItemStore {
            suffix: "/x",
            ..STORES[0]
        },
        // The same directory twice.
        ItemStore {
            suffix: "",
            ..STORES[0]
        },
    ] {
        let error = tree.open_for(&store, &[STORES[0], bad], false).err();
        assert_eq!(error, Some(CredentialError::InvalidName));
    }
    // A host that declares no item store opens the directory and nothing below it.
    let bare = tree.open_for(&store, &[], false).unwrap();
    assert_eq!(
        bare.store(OFFICE_KEYS).unwrap_err(),
        CredentialError::InvalidName
    );
    drop(bare);
    assert_eq!(
        refusal(tree.open_for(&store, &[], true)),
        (RootRule::CredentialsMissing, store.clone())
    );
    let opened = tree.open(&store).unwrap();

    // An item store that is replaced by a wrong one after the directory was opened is
    // still refused when it is opened: the engine checks it again.
    mkdir(&store.join("office-keys"), 0o755);
    assert_eq!(
        opened.store(OFFICE_KEYS).unwrap_err(),
        CredentialError::Unsafe
    );
    assert_eq!(mode(&store.join("office-keys")), 0o755);
    chmod(&store.join("office-keys"), 0o700);

    // Each view holds its own item store alone or shares it, as it asks.
    let exclusive = opened.store(HOST_SECRETS).unwrap();
    assert_eq!(
        opened.store(HOST_SECRETS).unwrap_err(),
        CredentialError::InUse
    );
    drop(exclusive);
    let shared = ReleaseItems {
        sharing: Sharing::Shared,
        ..OFFICE_KEYS
    };
    let (one, two) = (opened.store(shared).unwrap(), opened.store(shared).unwrap());
    one.add(&name("office-a"), b"key").unwrap();
    assert_eq!(loaded(&two, "office-a").unwrap(), b"key");
    // The two views are two directories: a name in one is not a name in the other.
    let secrets = opened.store(HOST_SECRETS).unwrap();
    assert_eq!(loaded(&secrets, "office-a"), None);
}

#[test]
fn a_moved_or_replaced_store_refuses_every_operation_of_its_views() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let opened = tree.open(&store).unwrap();
    let keys = opened.store(OFFICE_KEYS).unwrap();
    keys.add(&name("office-a"), b"key").unwrap();
    let moved = tree.base.join("moved");
    fs::rename(&store, &moved).unwrap();
    mkdir(&store, 0o700);
    mkdir(&store.join("office-keys"), 0o700);
    assert_eq!(
        keys.load(&name("office-a")).unwrap_err(),
        CredentialError::Unsafe
    );
    assert_eq!(
        keys.add(&name("office-b"), b"key").unwrap_err().kind,
        CredentialError::Unsafe
    );
    assert_eq!(
        fs::read(moved.join("office-keys/office-a.key")).unwrap(),
        b"key"
    );
    assert!(listing(&store.join("office-keys")).is_empty());
}

#[test]
fn a_refusal_names_the_directory_and_the_rule_and_a_location_prints_no_path() {
    let tree = Tree::new();
    let store = tree.dir("credentials", 0o750);
    let error = tree.open(&store).err().unwrap();
    let text = error.to_string();
    assert_eq!(
        text,
        format!(
            "{}: the credential directory's mode is not 0700",
            store.display()
        )
    );
    assert!(std::error::Error::source(&error).is_some());
    let data_root = tree.data_root();
    let location = ReleaseDirectory {
        path: &store,
        data_root: data_root.as_fd(),
        mount_point_required: true,
        expect_items: false,
    };
    let printed = format!("{location:?}");
    assert!(printed.contains("mount_point_required: true"), "{printed}");
    assert!(
        !printed.contains("credentials") && !printed.contains('/'),
        "{printed}"
    );
}

// ---------------------------------------------------------------------------------
// The host's item stores, judged when the directory opens.
// ---------------------------------------------------------------------------------

/// A store restored without its modes is reported as that: the item store is named
/// with its own rule, whether or not items are expected, and never as "credentials
/// missing".
#[test]
fn an_item_store_that_breaks_its_rule_is_named_before_the_store_is_called_empty() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let opened = tree.open(&store).unwrap();
    opened
        .store(OFFICE_KEYS)
        .unwrap()
        .add(&name("office-a"), b"key")
        .unwrap();
    drop(opened);
    let keys = store.join("office-keys");
    for wrong in [0o750, 0o755, 0o777, 0o500, 0o000] {
        chmod(&keys, wrong);
        for expect_items in [true, false] {
            assert_eq!(
                refusal(tree.open_for(&store, &STORES, expect_items)),
                (RootRule::ItemStoreMode, keys.clone()),
                "{wrong:o} {expect_items}"
            );
        }
        assert_eq!(mode(&keys), wrong, "the mode was repaired");
    }
    chmod(&keys, 0o700);
    drop(tree.open_expecting_items(&store).unwrap());
    let text = RootRule::ItemStoreMode.to_string();
    assert!(
        text.contains("item store") && text.contains("0700"),
        "{text}"
    );
    assert!(!text.contains("missing"), "{text}");

    // The other item store: a link, and a file, where its directory goes.
    let secrets = store.join("host-secrets");
    let elsewhere = tree.dir("elsewhere", 0o700);
    symlink(&elsewhere, &secrets).unwrap();
    assert_eq!(
        refusal(tree.open_expecting_items(&store)),
        (RootRule::ItemStoreNotADirectory, secrets.clone())
    );
    assert_eq!(
        refusal(tree.open(&store)),
        (RootRule::ItemStoreNotADirectory, secrets.clone())
    );
    assert!(listing(&elsewhere).is_empty());
    fs::remove_file(&secrets).unwrap();
    fs::write(&secrets, b"").unwrap();
    assert_eq!(
        refusal(tree.open(&store)),
        (RootRule::ItemStoreNotADirectory, secrets.clone())
    );
    fs::remove_file(&secrets).unwrap();
    // Another owner: only root can make such a directory.
    if is_root() {
        std::os::unix::fs::chown(&keys, Some(65534), Some(65534)).unwrap();
        assert_eq!(
            refusal(tree.open_expecting_items(&store)),
            (RootRule::ItemStoreOwner, keys.clone())
        );
        std::os::unix::fs::chown(&keys, Some(0), Some(0)).unwrap();
    }
    // Only the declared item stores are judged: the same directory is nothing to a
    // host that does not declare it.
    chmod(&keys, 0o755);
    drop(tree.open_for(&store, &STORES[1..], false).unwrap());
}

/// A file that is named like an item and is not what an item must be is not counted
/// as missing either: it refuses by its own rule, with its item store's path and
/// without its name.
#[test]
fn a_file_named_like_an_item_that_is_no_item_refuses_by_its_own_rule() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let opened = tree.open(&store).unwrap();
    drop((
        opened.store(OFFICE_KEYS).unwrap(),
        opened.store(HOST_SECRETS).unwrap(),
    ));
    drop(opened);
    let keys = store.join("office-keys");
    let secrets = store.join("host-secrets");
    let key = keys.join("office-a.key");
    let unsafe_keys = (RootRule::ItemUnsafe, keys.clone());

    // A key restored with another mode: the only item there is.
    fs::write(&key, b"key").unwrap();
    for wrong in [0o644, 0o640, 0o400, 0o700] {
        chmod(&key, wrong);
        assert_eq!(
            refusal(tree.open_expecting_items(&store)),
            unsafe_keys,
            "{wrong:o}"
        );
        assert_eq!(mode(&key), wrong, "the mode was repaired");
    }
    // A second link.
    chmod(&key, 0o600);
    drop(tree.open_expecting_items(&store).unwrap());
    fs::hard_link(&key, tree.base.join("second-name")).unwrap();
    assert_eq!(refusal(tree.open_expecting_items(&store)), unsafe_keys);
    fs::remove_file(tree.base.join("second-name")).unwrap();
    // A good item next to a bad one does not excuse the bad one.
    let other = keys.join("office-b.key");
    fs::write(&other, b"key").unwrap();
    chmod(&other, 0o644);
    assert_eq!(refusal(tree.open_expecting_items(&store)), unsafe_keys);
    fs::remove_file(&other).unwrap();
    fs::remove_file(&key).unwrap();
    // A link and a directory named like an item, in the other store.
    let victim = tree.base.join("victim");
    fs::write(&victim, b"keep").unwrap();
    chmod(&victim, 0o600);
    symlink(&victim, secrets.join("registry")).unwrap();
    assert_eq!(
        refusal(tree.open_expecting_items(&store)),
        (RootRule::ItemUnsafe, secrets.clone())
    );
    assert_eq!(fs::read(&victim).unwrap(), b"keep");
    fs::remove_file(secrets.join("registry")).unwrap();
    mkdir(&secrets.join("writer-device"), 0o700);
    assert_eq!(
        refusal(tree.open_expecting_items(&store)),
        (RootRule::ItemUnsafe, secrets.clone())
    );
    fs::remove_dir(secrets.join("writer-device")).unwrap();
    // The same file under a name that is no item's is nothing to the store.
    fs::write(keys.join("not-an-office.key"), b"x").unwrap();
    chmod(&keys.join("not-an-office.key"), 0o644);
    fs::write(secrets.join("README"), b"x").unwrap();
    assert_eq!(
        refusal(tree.open_expecting_items(&store)),
        (RootRule::CredentialsMissing, store.clone())
    );
    // The refusal names the item store and the rule, and no item.
    let error = {
        fs::write(&key, b"key").unwrap();
        chmod(&key, 0o644);
        tree.open_expecting_items(&store).err().unwrap().to_string()
    };
    assert!(
        error.starts_with(&format!("{}: ", keys.display())),
        "{error}"
    );
    assert!(!error.contains("office-a"), "{error}");
    // Without records nothing is counted, and the engine refuses the item at its use.
    let opened = tree.open(&store).unwrap();
    assert_eq!(
        opened
            .store(OFFICE_KEYS)
            .unwrap()
            .load(&name("office-a"))
            .unwrap_err(),
        CredentialError::Unsafe
    );
}

// ---------------------------------------------------------------------------------
// Access control lists on a directory admitted by a group-write exception.
// ---------------------------------------------------------------------------------

/// A directory above the store that its group can write is admitted only under the
/// private group (or, for root, sticky), and then only without an access ACL: an ACL
/// entry for another user makes that user a writer, and the mode shows it only as the
/// group's bits. The ACL is set with the attribute itself, as its owner may.
#[test]
fn a_group_writable_ancestor_with_an_access_acl_is_refused() {
    let Some(tree) = Tree::accepted() else { return };
    let parent = tree.dir("parent", 0o755);
    let store = parent.join("credentials");
    mkdir(&store, 0o700);
    let group = fs::symlink_metadata(&parent).unwrap().gid();
    if account::private_group_of(euid()).unwrap() != Some(group) {
        println!(
            "skipped: this user's group {group} is not its private group, so no \
             group-writable directory of it is admitted to begin with"
        );
        return;
    }
    // Another user, in no group of this one, gets write access by an ACL entry. The
    // mask is what the mode then shows for the group: the directory reads 0775.
    if let Err(error) = account::set_access_acl_for_test(&parent, 65534, 7) {
        println!("skipped: this filesystem takes no ACL: {error}");
        return;
    }
    assert_eq!(mode(&parent), 0o775);
    assert_eq!(
        refusal(tree.open(&store)),
        (RootRule::AncestorAcl, parent.clone())
    );
    assert!(listing(&store).is_empty());
    let text = RootRule::AncestorAcl.to_string();
    assert!(text.contains("access control list"), "{text}");
    // With a mask that denies writing, no entry of the ACL can write: the mode reads
    // 0755 again, the directory needs no exception, and the ACL is not looked at.
    account::set_access_acl_for_test(&parent, 65534, 5).unwrap();
    assert_eq!(mode(&parent), 0o755);
    drop(tree.open(&store).unwrap());
    // A plain group-writable directory in the private group, without an ACL.
    let plain = tree.dir("plain", 0o775);
    mkdir(&plain.join("credentials"), 0o700);
    drop(tree.open(&plain.join("credentials")).unwrap());
    // The store's own mode is exact, so an ACL on it cannot grant anything: with one,
    // its mode is no longer 0700 unless the mask is empty.
    account::set_access_acl_for_test(&plain.join("credentials"), 65534, 7).unwrap();
    assert_eq!(
        refusal(tree.open(&plain.join("credentials"))),
        (RootRule::StoreMode, plain.join("credentials"))
    );
}

// ---------------------------------------------------------------------------------
// Where a directory is inside its filesystem: bind mounts.
// ---------------------------------------------------------------------------------

/// The live probes of the mount check on this machine's real mount table, as any
/// user: a directory is placed by its mount and its path, a directory below it is
/// placed below it, another filesystem is another filesystem, and a directory that
/// was deleted has no place.
#[test]
fn a_directory_is_placed_in_its_filesystem_by_the_real_mount_table() {
    let bytes = mount_table().unwrap();
    let table = Table::of(&bytes).unwrap();
    println!(
        "this machine's mount table: {} bytes, {} mounts",
        bytes.len(),
        bytes.iter().filter(|byte| **byte == b'\n').count()
    );
    let tree = Tree::new();
    mkdir(&tree.base.join("a"), 0o700);
    mkdir(&tree.base.join("a/b with space"), 0o700);
    let at =
        |path: &Path| place(File::open(path).unwrap().as_fd(), &table).map(|(place, _mount)| place);
    let base = at(&tree.base).unwrap();
    let below = at(&tree.base.join("a/b with space")).unwrap();
    assert_eq!(below.filesystem, base.filesystem);
    let mut expected = base.path.clone();
    expected.push(b"a".to_vec());
    expected.push(b"b with space".to_vec());
    assert_eq!(below.path, expected);
    assert_eq!(
        mounts::disjoint(&below, &base),
        Err(RootRule::InsideDataRoot)
    );
    // A descriptor opened for passing through only is placed the same.
    let passing = open_absolute(&tree.base).unwrap();
    assert_eq!(place(passing.as_fd(), &table).unwrap().0, base);
    // `/proc` is another filesystem than the disk the tree is on.
    let proc_place = at(Path::new("/proc")).unwrap();
    assert_ne!(proc_place.filesystem, base.filesystem);
    assert!(proc_place.path.is_empty());
    assert_eq!(mounts::disjoint(&proc_place, &base), Ok(()));
    // A directory that is deleted while it is held has no path that names it.
    let doomed = tree.base.join("doomed");
    mkdir(&doomed, 0o700);
    let held = File::open(&doomed).unwrap();
    fs::remove_dir(&doomed).unwrap();
    assert_eq!(
        place(held.as_fd(), &table).err(),
        Some(MountDoubt::PathOutsideMount)
    );
    // And one that was renamed is placed where it is now.
    let moved = tree.base.join("moved");
    let held = File::open(tree.base.join("a")).unwrap();
    fs::rename(tree.base.join("a"), &moved).unwrap();
    let mut expected = base.path.clone();
    expected.push(b"moved".to_vec());
    assert_eq!(place(held.as_fd(), &table).unwrap().0.path, expected);
    // The mount a directory is in comes with its place: what it shows is the top of
    // the subtree the directory is in, and the directory's place is inside that.
    let (placed, mount) = place(File::open(&tree.base).unwrap().as_fd(), &table).unwrap();
    let volume = mounts::shown(&mount).unwrap();
    assert_eq!(volume.filesystem, placed.filesystem);
    assert!(placed.path.starts_with(&volume.path));
}

/// A bind mount, unmounted when it goes out of scope, so that a failed assertion
/// leaves none behind.
struct Bind(CString);

impl Drop for Bind {
    fn drop(&mut self) {
        // SAFETY: a NUL-terminated path; `MNT_DETACH` cannot block on a busy mount.
        unsafe { libc::umount2(self.0.as_ptr(), libc::MNT_DETACH) };
    }
}

/// Bind-mounts the directory `source` at `target`, or says why it could not.
fn bind(source: &Path, target: &Path) -> Result<Bind, std::io::Error> {
    let from = CString::new(source.as_os_str().as_bytes()).unwrap();
    let to = CString::new(target.as_os_str().as_bytes()).unwrap();
    // SAFETY: NUL-terminated paths that outlive the call; a bind mount reads no
    // filesystem type and no data.
    let mounted = unsafe {
        libc::mount(
            from.as_ptr(),
            to.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    if mounted != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Bind(to))
}

/// The reviewer's case on a real kernel, and its neighbours. A bind mount shows a
/// directory somewhere else, and its `..` leads out through the mount point: a store
/// that is a bind of a directory of the data root is nowhere near the data root by
/// the walk, and its keys would be in the data root. It is found inside the
/// filesystem, from the mount table. Making a mount takes root.
#[test]
fn a_bind_mount_cannot_hide_a_store_inside_the_data_root_or_the_reverse() {
    if !is_root() {
        println!(
            "skipped: only root can make a bind mount; the decision over mount records \
             (mounts.rs) covers the cases on every target"
        );
        return;
    }
    let Some(tree) = Tree::accepted() else { return };
    let mounted = |source: &Path, target: &Path| match bind(source, target) {
        Ok(bind) => Some(bind),
        Err(error) => {
            println!("skipped: this environment refuses a bind mount: {error}");
            None
        }
    };

    // 1. `mkdir data data/inner alias2; mount --bind data/inner alias2`: the store is
    //    `alias2`, the data root is `data`.
    let inner = tree.data.join("inner");
    mkdir(&inner, 0o700);
    let alias = tree.dir("alias2", 0o700);
    let Some(first) = mounted(&inner, &alias) else {
        return;
    };
    // The walk through `..` alone sees two unrelated directories.
    assert_eq!(
        rules::disjoint(
            &chain(File::open(&alias).unwrap().as_fd()).unwrap(),
            &chain(tree.data_root().as_fd()).unwrap()
        ),
        Ok(())
    );
    for mount_point_required in [false, true] {
        assert_eq!(
            refusal(tree.open_with(&alias, &tree.data_root(), mount_point_required, false)),
            (RootRule::InsideDataRoot, alias.clone())
        );
    }
    assert!(
        listing(&inner).is_empty(),
        "something was made in the data root"
    );
    // A directory below such a bind.
    mkdir(&inner.join("deeper"), 0o700);
    assert_eq!(
        refusal(tree.open(&alias.join("deeper"))),
        (RootRule::InsideDataRoot, alias.join("deeper"))
    );
    fs::remove_dir(inner.join("deeper")).unwrap();
    drop(first);

    // 2. The reverse: the data root is a bind of a directory of the store.
    let store = tree.dir("credentials", 0o700);
    mkdir(&store.join("sub"), 0o700);
    let data_bind = tree.dir("data2", 0o700);
    let Some(second) = mounted(&store.join("sub"), &data_bind) else {
        return;
    };
    let through = File::open(&data_bind).unwrap();
    assert_eq!(
        refusal(tree.open_with(&store, &through, false, false)),
        (RootRule::ContainsDataRoot, store.clone())
    );
    assert_eq!(listing(&store), ["sub"]);
    drop((through, second));
    fs::remove_dir(store.join("sub")).unwrap();

    // 3. The same directory twice, once through a bind; and a store inside a data
    //    root that is given through a bind alias. The walk refuses both already.
    let again = tree.dir("again", 0o700);
    let Some(third) = mounted(&store, &again) else {
        return;
    };
    let through = File::open(&again).unwrap();
    assert_eq!(
        refusal(tree.open_with(&store, &through, false, false)),
        (RootRule::IsDataRoot, store.clone())
    );
    drop((through, third));
    let data_alias = tree.dir("data-alias", 0o700);
    let Some(fourth) = mounted(&tree.data, &data_alias) else {
        return;
    };
    let inside = tree.data.join("credentials");
    mkdir(&inside, 0o700);
    let through = File::open(&data_alias).unwrap();
    assert_eq!(
        refusal(tree.open_with(&inside, &through, false, false)),
        (RootRule::InsideDataRoot, inside.clone())
    );
    // And the store reached through the alias, with the data root by its own path.
    assert_eq!(
        refusal(tree.open(&data_alias.join("credentials"))),
        (RootRule::InsideDataRoot, data_alias.join("credentials"))
    );
    drop((through, fourth));
    assert!(listing(&inside).is_empty());

    // 4. Two volumes, as a container has them: a bind of one directory where the
    //    data is, a bind of another where the secrets are, the instance's root a
    //    directory inside the data volume. Accepted, as a mount point and otherwise.
    let data_volume = tree.dir("data-volume", 0o700);
    mkdir(&data_volume.join("instance"), 0o700);
    let data_mount = tree.dir("container-data", 0o700);
    let Some(data_bound) = mounted(&data_volume, &data_mount) else {
        return;
    };
    let instance = File::open(data_mount.join("instance")).unwrap();
    let volume = tree.dir("volume", 0o700);
    let secrets = tree.dir("secrets", 0o700);
    let Some(fifth) = mounted(&volume, &secrets) else {
        return;
    };
    drop(tree.open_with(&secrets, &instance, false, false).unwrap());
    let opened = tree.open_with(&secrets, &instance, true, false).unwrap();
    opened
        .store(OFFICE_KEYS)
        .unwrap()
        .add(&name("office-a"), b"key")
        .unwrap();
    drop(opened);
    assert_eq!(
        fs::read(volume.join("office-keys/office-a.key")).unwrap(),
        b"key"
    );
    assert!(listing(&data_volume)
        .iter()
        .all(|entry| !entry.ends_with(".key")));
    // Where a mount point is required, a data root on the machine's own disk has all
    // of that disk as its volume, and a store that is a bind of a directory of the
    // same disk is in it. (No such store is asked for on an ordinary machine.)
    assert_eq!(
        refusal(tree.open_with(&secrets, &tree.data_root(), true, false)),
        (RootRule::InsideDataVolume, secrets.clone())
    );
    drop(
        tree.open_with(&secrets, &tree.data_root(), false, false)
            .unwrap(),
    );
    drop((fifth, instance, data_bound));

    // 4a. The container mistake with the instance's root below the data volume's top:
    //     `-v D:/data -v D/secrets:/secrets`, data root `/data/instance`. The store is
    //     beside the instance root, not inside it, and every key is in the data
    //     volume. As a mount point it is refused for the volume it is in.
    let agentic = tree.dir("agentic", 0o700);
    mkdir(&agentic.join("instance"), 0o700);
    mkdir(&agentic.join("secrets"), 0o700);
    let container_data = tree.dir("container-data-2", 0o700);
    let container_secrets = tree.dir("container-secrets-2", 0o700);
    let Some(data_bound) = mounted(&agentic, &container_data) else {
        return;
    };
    let Some(secrets_bound) = mounted(&agentic.join("secrets"), &container_secrets) else {
        return;
    };
    let instance = File::open(container_data.join("instance")).unwrap();
    // Neither the walk through `..` nor the comparison with the data root sees it.
    assert_eq!(
        rules::disjoint(
            &chain(File::open(&container_secrets).unwrap().as_fd()).unwrap(),
            &chain(instance.as_fd()).unwrap()
        ),
        Ok(())
    );
    assert_eq!(
        refusal(tree.open_with(&container_secrets, &instance, true, false)),
        (RootRule::InsideDataVolume, container_secrets.clone())
    );
    assert_eq!(listing(&agentic), ["instance", "secrets"]);
    // With the data root at the top of the volume it is inside the data root itself.
    let top = File::open(&container_data).unwrap();
    assert_eq!(
        refusal(tree.open_with(&container_secrets, &top, true, false)),
        (RootRule::InsideDataRoot, container_secrets.clone())
    );
    drop((top, instance, secrets_bound, data_bound));

    // 5. A mount whose directory was deleted: where it is cannot be said, so it is
    //    refused, not assumed to be elsewhere.
    let gone = tree.dir("gone", 0o700);
    let gone_mount = tree.dir("gone-mount", 0o700);
    let Some(sixth) = mounted(&gone, &gone_mount) else {
        return;
    };
    match fs::remove_dir(&gone) {
        Ok(()) => assert_eq!(
            refusal(tree.open(&gone_mount)),
            (
                RootRule::Undetermined(MountDoubt::DeletedRoot),
                gone_mount.clone()
            )
        ),
        Err(error) => println!("skipped: a mounted directory cannot be deleted here: {error}"),
    }
    drop(sixth);
    println!(
        "bind mounts judged on a real kernel: inside, reverse, alias, two volumes, secrets \
         in the data volume, deleted ({})",
        tree.base.display()
    );
}

/// A store on a read-only mount: the lock cannot be made, and the refusal says so
/// with the lock's own text, not as a part of the path that could not be opened.
/// Making a mount takes root.
#[test]
fn a_store_whose_lock_cannot_be_made_reports_the_lock() {
    if !is_root() {
        println!("skipped: only root can make a read-only mount");
        return;
    }
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("credentials", 0o700);
    let Ok(mount) = bind(&store, &store) else {
        println!("skipped: this environment refuses a bind mount");
        return;
    };
    // SAFETY: a NUL-terminated path; the flags remount this one bind read-only.
    let remounted = unsafe {
        libc::mount(
            std::ptr::null(),
            mount.0.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
            std::ptr::null(),
        )
    };
    if remounted != 0 {
        println!(
            "skipped: this environment refuses a read-only remount: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    let refused = tree.open(&store);
    drop(mount);
    assert_eq!(
        refusal(refused),
        (
            RootRule::LockUnavailable(ErrorKind::ReadOnlyFilesystem),
            store.clone()
        )
    );
    assert!(listing(&store).is_empty());
}

// ---------------------------------------------------------------------------------
// A deployment's own directories.
// ---------------------------------------------------------------------------------

/// The directories a deployment names, judged exactly as its host judges them: from
/// `/`, through the contract, as a store that must be a mount point. It is for a run
/// inside a container, where the mount table is the container's own and the two
/// volumes are what the operator mounted:
///
/// ```text
/// ORG_RELEASE_DIRECTORY_PROBE_STORE=/secrets
/// ORG_RELEASE_DIRECTORY_PROBE_DATA_ROOT=/data/instance
/// ORG_RELEASE_DIRECTORY_PROBE_EXPECT=accepted        (or a rule's name: InsideDataVolume)
/// ```
///
/// Without the three variables it runs nothing. Accepted, it adds a key through the
/// office keys' item store and loads it through a second handle.
#[test]
fn the_directories_a_deployment_names_are_judged_as_its_host_judges_them() {
    let variable = |name: &str| std::env::var_os(format!("ORG_RELEASE_DIRECTORY_PROBE_{name}"));
    let (Some(store), Some(data_root), Some(expected)) =
        (variable("STORE"), variable("DATA_ROOT"), variable("EXPECT"))
    else {
        println!("skipped: no deployment is named (ORG_RELEASE_DIRECTORY_PROBE_*)");
        return;
    };
    let (store, data_root) = (PathBuf::from(store), PathBuf::from(data_root));
    let expected = expected.to_string_lossy().into_owned();
    let bytes = mount_table().unwrap();
    println!(
        "probe: uid {}, mount table of {} mounts, store {}, data root {}",
        euid(),
        bytes.iter().filter(|byte| **byte == b'\n').count(),
        store.display(),
        data_root.display()
    );
    let data = File::open(&data_root).unwrap();
    let location = ReleaseDirectory {
        path: &store,
        data_root: data.as_fd(),
        mount_point_required: true,
        expect_items: false,
    };
    match contract(location, &STORES) {
        Ok(opened) => {
            println!("probe: accepted");
            assert_eq!(expected, "accepted");
            let keys = opened.store(OFFICE_KEYS).unwrap();
            let item = name("probe-office");
            keys.delete(&item).unwrap();
            keys.add(&item, b"probe key").unwrap();
            drop((keys, opened));
            let again = contract(
                ReleaseDirectory {
                    expect_items: true,
                    ..location
                },
                &STORES,
            )
            .unwrap();
            assert_eq!(
                loaded(&again.store(OFFICE_KEYS).unwrap(), "probe-office").unwrap(),
                b"probe key"
            );
            // Nothing of it is in the data root.
            assert!(listing(&data_root)
                .iter()
                .all(|entry| !entry.ends_with(".key")));
            println!("probe: a key was added and loaded through a second handle");
        }
        Err(CredentialError::Root(refused)) => {
            println!("probe: refused: {refused}");
            assert_eq!(format!("{:?}", refused.rule), expected);
        }
        Err(other) => panic!("probe: {other:?}"),
    }
}

/// X5 review (third round), P1: a development build's key folder is one folder directly in
/// the release directory, made owner-only from its descriptor when it is missing, judged
/// without following a link, and its stores are opened from the checked folder and hold
/// the release directory's lock. Another volume mounted on the folder is refused by the
/// mount rule (`rules.rs`, and LNX-2 with a real nested mount); this test runs as an
/// ordinary user, who cannot mount one.
#[test]
fn a_development_folder_is_made_and_checked_in_the_release_directory_and_holds_its_lock() {
    let Some(tree) = Tree::accepted() else { return };
    let store = tree.dir("secrets", 0o700);
    let opened = tree.open(&store).unwrap();
    let folder = opened.development_folder("dev-keys").unwrap();
    assert_eq!(mode(&store.join("dev-keys")), 0o700);
    let secrets = folder
        .store(&["host-secrets", "instance"], Sharing::Exclusive, "")
        .unwrap();
    secrets.add(&name("registry"), b"registry key").unwrap();
    assert_eq!(
        fs::read(store.join("dev-keys/host-secrets/instance/registry")).unwrap(),
        b"registry key"
    );
    // The folder and its store hold the release directory: no second host opens it, until
    // the last of them is gone.
    drop((folder, opened));
    assert_eq!(refusal(tree.open(&store)), (RootRule::InUse, store.clone()));
    drop(secrets);
    let opened = tree.open(&store).unwrap();
    // Found again, as it is.
    let folder = opened.development_folder("dev-keys").unwrap();
    assert_eq!(
        loaded(
            &folder
                .store(&["host-secrets", "instance"], Sharing::Exclusive, "")
                .unwrap(),
            "registry"
        )
        .unwrap(),
        b"registry key"
    );
    drop(folder);

    // Not a folder name: an item store's own directory, the lock, a path, nothing.
    let before = listing(&store);
    for name in [
        "office-keys",
        "host-secrets",
        ".lock",
        "a/b",
        "",
        "lost+found",
        "..",
    ] {
        assert_eq!(
            opened.development_folder(name).err(),
            Some(CredentialError::InvalidName),
            "{name}"
        );
    }
    assert_eq!(listing(&store), before);

    // A link to a folder elsewhere is not followed, and nothing is made there; nor is a
    // file taken for a folder.
    let elsewhere = tree.dir("elsewhere", 0o700);
    symlink(&elsewhere, store.join("linked")).unwrap();
    fs::write(store.join("plain"), b"").unwrap();
    for name in ["linked", "plain"] {
        assert_eq!(
            refusal(opened.development_folder(name)),
            (RootRule::DevelopmentFolderNotADirectory, store.join(name)),
            "{name}"
        );
    }
    assert!(listing(&elsewhere).is_empty());

    // A folder open to its group is refused and left as it is.
    let open = store.join("open");
    mkdir(&open, 0o750);
    assert_eq!(
        refusal(opened.development_folder("open")),
        (RootRule::DevelopmentFolderMode, open.clone())
    );
    assert_eq!(mode(&open), 0o750);
}
