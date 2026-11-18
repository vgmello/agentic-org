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
    release_directory as contract, Credentials, FilesystemClass, ItemName, Sharing, MAX_ITEM_BYTES,
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
fn refusal(result: Result<Opened, CredentialError>) -> (RootRule, PathBuf) {
    match result {
        Err(CredentialError::Root(RootRefusal { rule, path })) => (rule, path),
        Err(other) => panic!("refused with {other:?}, not with a rule of the release directory"),
        Ok(_) => panic!("the release directory was opened"),
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
    contract(ReleaseDirectory {
        path,
        data_root: data_root.as_fd(),
        mount_point_required: false,
        expect_items,
    })
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
/// accepted when one is required. Making a mount takes root (and a container that
/// allows it); otherwise the reason is printed and the probe test above, with the
/// decision's tests, carries the rule.
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

    // What is not an item does not make the store hold one: a temporary file, a
    // directory and a link inside a view; a file next to the views; a directory that
    // cannot be a view (another mode) with a file in it; an unreadable directory.
    fs::write(
        store.join("office-keys/.put.0123456789abcdef"),
        b"temporary",
    )
    .unwrap();
    mkdir(&store.join("office-keys/a-directory"), 0o700);
    symlink("/etc/hostname", store.join("host-secrets/a-link")).unwrap();
    fs::write(store.join("stray-file"), b"stray").unwrap();
    mkdir(&store.join("open-directory"), 0o755);
    fs::write(store.join("open-directory/looks-like-an-item"), b"x").unwrap();
    mkdir(&store.join("lost+found"), 0o700);
    fs::write(store.join("lost+found/x"), b"x").unwrap();
    chmod(&store.join("lost+found"), 0o000);
    assert_eq!(refusal(tree.open_expecting_items(&store)), missing);
    chmod(&store.join("lost+found"), 0o700);
    fs::remove_file(store.join("lost+found/x")).unwrap();

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
        if wrong & 0o200 == 0 && !is_root() {
            // Not writable by its owner: it cannot be opened for the lock at all.
            assert_eq!(
                refusal(tree.open(&store)),
                (
                    RootRule::Unavailable(ErrorKind::PermissionDenied),
                    store.clone()
                )
            );
        } else {
            assert_eq!(refusal(tree.open(&store)), refused, "{wrong:o}");
        }
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
    // Not a single plain name, or a suffix that is none: refused, and nothing is made.
    for directory in [
        "",
        ".",
        "..",
        ".hidden",
        "a/b",
        "../outside",
        "a b",
        ".lock",
    ] {
        assert_eq!(
            opened.store(items(directory, "")).unwrap_err(),
            CredentialError::InvalidName,
            "{directory:?}"
        );
    }
    assert_eq!(
        opened.store(items("keys", "/x")).unwrap_err(),
        CredentialError::InvalidName
    );
    assert_eq!(listing(&store), [".lock"]);
    assert!(!tree.base.join("outside").exists());

    // An item store's directory that exists with another mode, or is a link.
    mkdir(&store.join("open"), 0o755);
    assert_eq!(
        opened.store(items("open", "")).unwrap_err(),
        CredentialError::Unsafe
    );
    assert_eq!(mode(&store.join("open")), 0o755);
    let elsewhere = tree.dir("elsewhere", 0o700);
    symlink(&elsewhere, store.join("linked")).unwrap();
    assert!(opened.store(items("linked", "")).is_err());
    assert!(listing(&elsewhere).is_empty());

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
