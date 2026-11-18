//! The rules of a release directory, pure: each decision over plain facts.
//!
//! The GNU/Linux adapter supplies the facts from live probes (`linux/release.rs`) and
//! is the only caller; the decisions are compiled and tested on every target. The
//! rules are stated in `release.rs`.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use super::release::{FilesystemClass, RootRule};

/// The permission bits of a mode, with the set-id and sticky bits.
const PERMISSIONS: u32 = 0o7777;
const STICKY: u32 = 0o1000;
const GROUP_WRITE: u32 = 0o020;
const OTHERS_WRITE: u32 = 0o002;

/// The single names of `path` below `/`, in order: none for `/` itself. `.` and
/// repeated separators are spelling and are dropped; a relative path, a `..` and a
/// NUL byte are refused before anything is opened.
pub(super) fn components(path: &Path) -> Result<Vec<&OsStr>, RootRule> {
    if !path.is_absolute() {
        return Err(RootRule::Relative);
    }
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => names.push(name),
            Component::ParentDir => return Err(RootRule::ParentComponent),
            // A Unix path has no prefix.
            Component::Prefix(_) => return Err(RootRule::Relative),
        }
    }
    if path.as_os_str().as_bytes().contains(&0) {
        return Err(RootRule::NotAPath);
    }
    Ok(names)
}

/// What is known of one directory above the release directory.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ancestor {
    /// The directory's owner.
    pub(super) owner: u32,
    /// The directory's group.
    pub(super) group: u32,
    /// The directory's mode (the file type bits are not read).
    pub(super) mode: u32,
    /// The service user.
    pub(super) service_user: u32,
    /// The service user's private group, if it has one.
    pub(super) private_group: Option<u32>,
}

/// Whether judging a directory of this `owner` and `mode` needs the service user's
/// private group: only a group-writable directory of the service user does. The
/// adapter asks the account database only then.
pub(super) fn needs_private_group(owner: u32, mode: u32, service_user: u32) -> bool {
    owner == service_user && mode & GROUP_WRITE != 0
}

/// The rule of a directory above the release directory: owned by root or the service
/// user, never writable by others, and writable by its group only if it is a
/// root-owned sticky directory or a directory of the service user whose group is that
/// user's private group.
pub(super) fn ancestor(facts: &Ancestor) -> Result<(), RootRule> {
    let Ancestor {
        owner,
        group,
        mode,
        service_user,
        private_group,
    } = *facts;
    if owner != 0 && owner != service_user {
        return Err(RootRule::AncestorOwner);
    }
    if mode & OTHERS_WRITE != 0 {
        return Err(RootRule::AncestorWritableByOthers);
    }
    if mode & GROUP_WRITE != 0 {
        let root_sticky = owner == 0 && mode & STICKY != 0;
        // A root-owned directory is root's, whoever the service user is: when the
        // service user is root, only its own private group opens a second way.
        let private = owner == service_user && private_group == Some(group);
        if !root_sticky && !private {
            return Err(RootRule::AncestorWritableByGroup);
        }
    }
    Ok(())
}

/// The rule of the release directory itself: the service user's, with mode exactly
/// `0700` (no set-id or sticky bit either).
pub(super) fn store(owner: u32, mode: u32, service_user: u32) -> Result<(), RootRule> {
    if owner != service_user {
        return Err(RootRule::StoreOwner);
    }
    if mode & PERMISSIONS != 0o700 {
        return Err(RootRule::StoreMode);
    }
    Ok(())
}

/// The rule of the lock file: a regular file (`regular`) of the service user with mode
/// exactly `0600` and one link.
pub(super) fn lock_file(
    regular: bool,
    owner: u32,
    mode: u32,
    links: u64,
    service_user: u32,
) -> Result<(), RootRule> {
    if regular && owner == service_user && mode & PERMISSIONS == 0o600 && links == 1 {
        Ok(())
    } else {
        Err(RootRule::LockFile)
    }
}

// Filesystem type numbers, as `statfs` gives them (`<linux/magic.h>` and the
// filesystems' own headers). They are plain numbers here so that the decision is the
// same code on every target.
const EXT4: u64 = 0xef53; // also ext2 and ext3
const XFS: u64 = 0x5846_5342;
const BTRFS: u64 = 0x9123_683e;
const F2FS: u64 = 0xf2f5_2010;

/// Network and cluster filesystems.
const NETWORK: &[u64] = &[
    0x6969,      // NFS
    0x517b,      // SMB
    0xff53_4d42, // CIFS
    0xfe53_4d42, // SMB2 and later
    0x0102_1997, // 9p
    0x00c3_6400, // Ceph
    0x5346_414f, // AFS (OpenAFS)
    0x6b41_4653, // AFS (kAFS)
    0x7375_7245, // Coda
    0x564c,      // NCP
    0x7461_636f, // OCFS2
    0x0116_1970, // GFS2
    0x0bd0_0bd0, // Lustre
    0x4750_4653, // GPFS
    0xaad7_aaea, // PanFS
    0x1983_0326, // BeeGFS
    0x2003_0528, // OrangeFS (pvfs2)
    0x786f_4256, // VirtualBox shared folders
    0xbacb_acbc, // VMware host-guest filesystem
];

/// User-space filesystems: everything FUSE serves reports FUSE's number (sshfs,
/// virtiofs, Docker Desktop's file sharing).
const USER_SPACE: &[u64] = &[
    0x6573_5546, // FUSE
    0x6573_5543, // the FUSE control filesystem
];

/// Memory filesystems.
const MEMORY: &[u64] = &[
    0x0102_1994, // tmpfs, and devtmpfs
    0x8584_58f6, // ramfs
    0x9584_58f6, // hugetlbfs
];

/// The rule of the release directory's filesystem, over the type number `statfs`
/// gives: ext4, XFS, Btrfs or F2FS, and nothing else.
pub(super) fn filesystem(kind: u64) -> Result<(), RootRule> {
    if [EXT4, XFS, BTRFS, F2FS].contains(&kind) {
        return Ok(());
    }
    let class = if NETWORK.contains(&kind) {
        FilesystemClass::Network
    } else if USER_SPACE.contains(&kind) {
        FilesystemClass::UserSpace
    } else if MEMORY.contains(&kind) {
        FilesystemClass::Memory
    } else {
        FilesystemClass::Other
    };
    Err(RootRule::Filesystem { class, kind })
}

/// A directory's device and inode numbers.
pub(super) type Identity = (u64, u64);

/// The rule that keeps the release directory and the data root apart. Each chain is a
/// directory first and then every directory above it up to `/`, as held descriptors
/// identified them: neither directory is the other, and neither is above the other.
/// A chain that is empty names no directory and proves nothing: refused.
pub(super) fn disjoint(store: &[Identity], data_root: &[Identity]) -> Result<(), RootRule> {
    let (Some((store_dir, above_store)), Some((data_dir, above_data))) =
        (store.split_first(), data_root.split_first())
    else {
        return Err(RootRule::DataRoot(std::io::ErrorKind::InvalidInput));
    };
    if store_dir == data_dir {
        return Err(RootRule::IsDataRoot);
    }
    if above_store.contains(data_dir) {
        return Err(RootRule::InsideDataRoot);
    }
    if above_data.contains(store_dir) {
        return Err(RootRule::ContainsDataRoot);
    }
    Ok(())
}

/// `STATX_ATTR_MOUNT_ROOT` (`<linux/stat.h>`, Linux 5.8).
pub(super) const MOUNT_ROOT: u64 = 0x2000;

/// The mount-point rule, over what `statx` answered for the release directory: its
/// attributes and the mask of the attributes this kernel reports. A kernel that does
/// not report the attribute cannot vouch for a mount point.
pub(super) fn mount_point(attributes: u64, reported: u64) -> Result<(), RootRule> {
    if reported & MOUNT_ROOT == 0 {
        return Err(RootRule::MountPointUnknown);
    }
    if attributes & MOUNT_ROOT == 0 {
        return Err(RootRule::NotAMountPoint);
    }
    Ok(())
}

/// The empty-store rule: when the host's records say keys exist (`expected`), the
/// release directory must hold at least one item.
pub(super) fn items_present(expected: bool, items: usize) -> Result<(), RootRule> {
    if expected && items == 0 {
        Err(RootRule::CredentialsMissing)
    } else {
        Ok(())
    }
}

/// Whether the directory entry `name` counts as an item for the empty-store rule: any
/// name that does not start with `.` (the lock and temporary files do). The caller
/// has established that the entry is a regular file in an item store's directory.
pub(super) fn counts_as_item(name: &[u8]) -> bool {
    !name.is_empty() && name[0] != b'.'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::os::unix::ffi::OsStrExt;

    const USER: u32 = 1000;
    const GROUP: u32 = 1000;

    #[test]
    fn a_path_is_absolute_without_parent_components_and_spelling_is_dropped() {
        let names = |path: &str| {
            components(Path::new(path)).map(|names| {
                names
                    .iter()
                    .map(|n| n.to_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(names("/"), Ok(vec![]));
        assert_eq!(names("/secrets"), Ok(vec!["secrets".to_owned()]));
        assert_eq!(
            names("/home/a/.local/state/agentic-org/credentials/0197"),
            Ok([
                "home",
                "a",
                ".local",
                "state",
                "agentic-org",
                "credentials",
                "0197"
            ]
            .map(str::to_owned)
            .to_vec())
        );
        // Spelling only: repeated and trailing separators, and `.`.
        assert_eq!(
            names("//a/./b//c/."),
            Ok(["a", "b", "c"].map(str::to_owned).to_vec())
        );
        for relative in ["", "secrets", "./secrets", "a/b", "~/x"] {
            assert_eq!(names(relative), Err(RootRule::Relative), "{relative:?}");
        }
        for traversal in ["/..", "/a/../b", "/a/b/..", "/a/./../b"] {
            assert_eq!(
                names(traversal),
                Err(RootRule::ParentComponent),
                "{traversal:?}"
            );
        }
        // A relative path with `..` is refused for being relative, first.
        assert_eq!(names("../a"), Err(RootRule::Relative));
        let with_nul = Path::new(OsStr::from_bytes(b"/a\0b"));
        assert_eq!(components(with_nul), Err(RootRule::NotAPath));
        // A name that only looks like a parent component is a name.
        assert_eq!(names("/a/..b/c..").unwrap().len(), 3);
    }

    fn judge(
        owner: u32,
        group: u32,
        mode: u32,
        private_group: Option<u32>,
    ) -> Result<(), RootRule> {
        ancestor(&Ancestor {
            owner,
            group,
            mode,
            service_user: USER,
            private_group,
        })
    }

    #[test]
    fn an_ancestor_is_roots_or_the_service_users_and_closed_to_others() {
        for mode in [0o755, 0o700, 0o711, 0o750, 0o555, 0o1755, 0o2755] {
            assert_eq!(judge(0, 0, mode, None), Ok(()), "root {mode:o}");
            assert_eq!(judge(USER, GROUP, mode, None), Ok(()), "user {mode:o}");
        }
        for owner in [1, 999, 1001, 65534] {
            for mode in [0o700, 0o755, 0o1777] {
                assert_eq!(
                    judge(owner, GROUP, mode, Some(GROUP)),
                    Err(RootRule::AncestorOwner),
                    "{owner} {mode:o}"
                );
            }
        }
        // Writable by others: never, sticky or not, whoever owns it and whatever its
        // group is. `/tmp` is such a directory.
        for mode in [0o777, 0o1777, 0o757, 0o702, 0o1703, 0o773] {
            for (owner, group) in [(0, 0), (USER, GROUP)] {
                assert_eq!(
                    judge(owner, group, mode, Some(GROUP)),
                    Err(RootRule::AncestorWritableByOthers),
                    "{owner} {mode:o}"
                );
            }
        }
    }

    #[test]
    fn an_ancestor_is_group_writable_only_as_a_root_sticky_directory_or_under_the_private_group() {
        let refused = Err(RootRule::AncestorWritableByGroup);
        // Root-owned: only with the sticky bit, whatever the group.
        for group in [0, 4, GROUP] {
            assert_eq!(judge(0, group, 0o1775, None), Ok(()), "{group}");
            assert_eq!(judge(0, group, 0o1770, Some(GROUP)), Ok(()), "{group}");
            assert_eq!(judge(0, group, 0o775, None), refused, "{group}");
            // Root's directory in the service user's private group is still root's.
            assert_eq!(judge(0, group, 0o775, Some(group)), refused, "{group}");
            assert_eq!(judge(0, group, 0o2775, Some(GROUP)), refused, "{group}");
        }
        // The service user's: only when the directory's group is its private group.
        for mode in [0o775, 0o770, 0o2775, 0o1775] {
            assert_eq!(judge(USER, GROUP, mode, Some(GROUP)), Ok(()), "{mode:o}");
            // No private group (a shared primary group, or no account entry).
            assert_eq!(judge(USER, GROUP, mode, None), refused, "{mode:o}");
            // Another group than the private one.
            assert_eq!(judge(USER, 27, mode, Some(GROUP)), refused, "{mode:o}");
        }
        // The sticky bit excuses root's directories only.
        assert_eq!(judge(USER, 27, 0o1775, None), refused);
        // The private group never excuses others' write access or another owner.
        assert_eq!(
            judge(USER, GROUP, 0o777, Some(GROUP)),
            Err(RootRule::AncestorWritableByOthers)
        );
        assert_eq!(
            judge(1001, GROUP, 0o775, Some(GROUP)),
            Err(RootRule::AncestorOwner)
        );
    }

    #[test]
    fn root_as_the_service_user_keeps_both_exceptions_apart() {
        let as_root = |group, mode, private_group| {
            ancestor(&Ancestor {
                owner: 0,
                group,
                mode,
                service_user: 0,
                private_group,
            })
        };
        assert_eq!(as_root(0, 0o775, Some(0)), Ok(()));
        assert_eq!(as_root(4, 0o1775, Some(0)), Ok(()));
        assert_eq!(
            as_root(4, 0o775, Some(0)),
            Err(RootRule::AncestorWritableByGroup)
        );
        assert_eq!(
            as_root(0, 0o775, None),
            Err(RootRule::AncestorWritableByGroup)
        );
    }

    #[test]
    fn the_private_group_is_asked_for_only_when_it_can_decide() {
        assert!(needs_private_group(USER, 0o775, USER));
        assert!(needs_private_group(USER, 0o720, USER));
        for (owner, mode) in [(USER, 0o755), (USER, 0o700), (0, 0o775), (1001, 0o775)] {
            assert!(!needs_private_group(owner, mode, USER), "{owner} {mode:o}");
        }
    }

    #[test]
    fn the_store_is_the_service_users_with_mode_0700_exactly() {
        assert_eq!(store(USER, 0o700, USER), Ok(()));
        // The file type bits are not part of the mode that is judged.
        assert_eq!(store(USER, 0o040_700, USER), Ok(()));
        for mode in [
            0o750, 0o600, 0o777, 0o755, 0o711, 0o701, 0o710, 0o500, 0o000, 0o1700, 0o2700, 0o4700,
        ] {
            assert_eq!(
                store(USER, mode, USER),
                Err(RootRule::StoreMode),
                "{mode:o}"
            );
        }
        for owner in [0, 1001, 65534] {
            assert_eq!(store(owner, 0o700, USER), Err(RootRule::StoreOwner));
        }
        // The owner is judged first.
        assert_eq!(store(0, 0o777, USER), Err(RootRule::StoreOwner));
    }

    #[test]
    fn the_lock_is_a_regular_owner_only_file_with_one_link() {
        assert_eq!(lock_file(true, USER, 0o600, 1, USER), Ok(()));
        assert_eq!(lock_file(true, USER, 0o100_600, 1, USER), Ok(()));
        for (regular, owner, mode, links) in [
            (false, USER, 0o600, 1),
            (true, 0, 0o600, 1),
            (true, 1001, 0o600, 1),
            (true, USER, 0o644, 1),
            (true, USER, 0o660, 1),
            (true, USER, 0o400, 1),
            (true, USER, 0o700, 1),
            (true, USER, 0o600, 2),
            (true, USER, 0o600, 0),
        ] {
            assert_eq!(
                lock_file(regular, owner, mode, links, USER),
                Err(RootRule::LockFile),
                "{regular} {owner} {mode:o} {links}"
            );
        }
    }

    #[test]
    fn only_the_four_disk_filesystems_hold_a_store_and_every_refusal_names_its_class() {
        for accepted in [0xef53, 0x5846_5342, 0x9123_683e, 0xf2f5_2010] {
            assert_eq!(filesystem(accepted), Ok(()), "{accepted:#x}");
        }
        let class_of = |kind| match filesystem(kind) {
            Err(RootRule::Filesystem { class, kind: found }) => {
                assert_eq!(found, kind);
                class
            }
            other => panic!("{kind:#x} was not refused as a filesystem: {other:?}"),
        };
        for (name, kind, class) in [
            ("NFS", 0x6969, FilesystemClass::Network),
            ("SMB", 0x517b, FilesystemClass::Network),
            ("CIFS", 0xff53_4d42, FilesystemClass::Network),
            ("SMB2", 0xfe53_4d42, FilesystemClass::Network),
            ("9p", 0x0102_1997, FilesystemClass::Network),
            ("Ceph", 0x00c3_6400, FilesystemClass::Network),
            (
                "FUSE (sshfs, virtiofs)",
                0x6573_5546,
                FilesystemClass::UserSpace,
            ),
            ("tmpfs", 0x0102_1994, FilesystemClass::Memory),
            ("ramfs", 0x8584_58f6, FilesystemClass::Memory),
            ("hugetlbfs", 0x9584_58f6, FilesystemClass::Memory),
            ("overlayfs", 0x794c_7630, FilesystemClass::Other),
            ("ZFS", 0x2fc1_2fc1, FilesystemClass::Other),
            ("squashfs", 0x7371_7368, FilesystemClass::Other),
            ("proc", 0x9fa0, FilesystemClass::Other),
            ("APFS has no such number", 0x1a, FilesystemClass::Other),
            ("nothing", 0, FilesystemClass::Other),
        ] {
            assert_eq!(class_of(kind), class, "{name}");
        }
        // The text of every refusal names the three refused classes and the four
        // accepted filesystems, and what was found.
        for (kind, found) in [
            (0x6969, "a network filesystem"),
            (0x6573_5546, "a user-space filesystem"),
            (0x0102_1994, "a memory filesystem"),
            (0x794c_7630, "another filesystem"),
        ] {
            let text = filesystem(kind).unwrap_err().to_string();
            for part in [
                "ext4",
                "XFS",
                "Btrfs",
                "F2FS",
                "network",
                "user-space",
                "memory",
                found,
                &format!("{kind:#x}"),
            ] {
                assert!(text.contains(part), "{text}: {part}");
            }
        }
    }

    #[test]
    fn the_store_and_the_data_root_are_neither_the_same_nor_inside_each_other() {
        // (device, inode); `/` is (1, 2).
        let root = (1, 2);
        let home = (1, 10);
        let data = (1, 20);
        let store_dir = (1, 30);
        // Siblings below one home, and directories on two devices.
        assert_eq!(
            disjoint(&[store_dir, home, root], &[data, home, root]),
            Ok(())
        );
        assert_eq!(disjoint(&[(2, 2), root], &[(3, 2), root]), Ok(()));
        // The same inode number on another device is another directory.
        assert_eq!(disjoint(&[(2, 20), root], &[data, home, root]), Ok(()));
        // The same directory, however each was reached.
        assert_eq!(
            disjoint(&[data, home, root], &[data, home, root]),
            Err(RootRule::IsDataRoot)
        );
        // The store inside the data root, directly and deeper.
        assert_eq!(
            disjoint(&[store_dir, data, home, root], &[data, home, root]),
            Err(RootRule::InsideDataRoot)
        );
        assert_eq!(
            disjoint(&[store_dir, (1, 31), data, home, root], &[data, home, root]),
            Err(RootRule::InsideDataRoot)
        );
        // The data root inside the store.
        assert_eq!(
            disjoint(
                &[store_dir, home, root],
                &[data, (1, 21), store_dir, home, root]
            ),
            Err(RootRule::ContainsDataRoot)
        );
        // `/` as either one contains or is the other.
        assert_eq!(
            disjoint(&[root], &[data, home, root]),
            Err(RootRule::ContainsDataRoot)
        );
        assert_eq!(
            disjoint(&[store_dir, home, root], &[root]),
            Err(RootRule::InsideDataRoot)
        );
        assert_eq!(disjoint(&[root], &[root]), Err(RootRule::IsDataRoot));
        // A chain that names no directory proves nothing.
        for (store, data_root) in [
            (&[][..], &[data, root][..]),
            (&[store_dir, root][..], &[][..]),
        ] {
            assert_eq!(
                disjoint(store, data_root),
                Err(RootRule::DataRoot(ErrorKind::InvalidInput))
            );
        }
    }

    #[test]
    fn a_required_mount_point_is_one_the_kernel_vouches_for() {
        assert_eq!(mount_point(MOUNT_ROOT, MOUNT_ROOT), Ok(()));
        assert_eq!(mount_point(MOUNT_ROOT | 0x10, MOUNT_ROOT | 0x30), Ok(()));
        assert_eq!(mount_point(0, MOUNT_ROOT), Err(RootRule::NotAMountPoint));
        assert_eq!(
            mount_point(0x10, MOUNT_ROOT | 0x10),
            Err(RootRule::NotAMountPoint)
        );
        // A kernel that does not report the attribute: unknown, whatever the bits say.
        assert_eq!(mount_point(0, 0), Err(RootRule::MountPointUnknown));
        assert_eq!(mount_point(MOUNT_ROOT, 0), Err(RootRule::MountPointUnknown));
        assert_eq!(MOUNT_ROOT, 0x2000);
    }

    #[test]
    fn an_empty_store_is_refused_only_when_the_records_say_keys_exist() {
        assert_eq!(items_present(true, 0), Err(RootRule::CredentialsMissing));
        assert_eq!(items_present(true, 1), Ok(()));
        assert_eq!(items_present(false, 0), Ok(()));
        assert_eq!(items_present(false, 7), Ok(()));
        for item in ["01AAAA.key", "registry", "writer-device", "a"] {
            assert!(counts_as_item(item.as_bytes()), "{item}");
        }
        for other in ["", ".lock", ".put.0123456789abcdef", ".", ".."] {
            assert!(!counts_as_item(other.as_bytes()), "{other}");
        }
    }

    #[test]
    fn every_rule_has_its_own_text_and_no_text_runs_over_a_line() {
        let rules = [
            RootRule::Relative,
            RootRule::ParentComponent,
            RootRule::NotAPath,
            RootRule::NotADirectory,
            RootRule::Unavailable(ErrorKind::NotFound),
            RootRule::AncestorOwner,
            RootRule::AncestorWritableByOthers,
            RootRule::AncestorWritableByGroup,
            RootRule::StoreOwner,
            RootRule::StoreMode,
            RootRule::Filesystem {
                class: FilesystemClass::Memory,
                kind: 0x0102_1994,
            },
            RootRule::IsDataRoot,
            RootRule::InsideDataRoot,
            RootRule::ContainsDataRoot,
            RootRule::DataRoot(ErrorKind::PermissionDenied),
            RootRule::NotAMountPoint,
            RootRule::MountPointUnknown,
            RootRule::CredentialsMissing,
            RootRule::LockFile,
            RootRule::InUse,
        ];
        let texts: Vec<String> = rules.iter().map(ToString::to_string).collect();
        for (i, text) in texts.iter().enumerate() {
            assert!(!text.is_empty() && !text.contains('\n'), "{text}");
            assert!(!texts[..i].contains(text), "two rules read alike: {text}");
        }
        assert!(
            texts[17].starts_with("credentials missing"),
            "{}",
            texts[17]
        );
    }
}
