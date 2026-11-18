//! Where a directory is inside its filesystem, from the mount table: pure.
//!
//! The walk up through `..` (`rules::disjoint`) compares the release directory and the
//! data root as this process sees them. A bind mount hides what it shows: the parent
//! of a mount's root is the parent of the mount *point*, so a directory of the data
//! root that is bind-mounted elsewhere is, by that walk, nowhere near the data root.
//! (`docker run -v ./agentic:/data -v ./agentic/secrets:/secrets` is that mistake:
//! every key would be in the data volume.) This module answers the question the walk
//! cannot: where each directory is inside its own filesystem.
//!
//! The kernel's mount table (`/proc/self/mountinfo`) names, for every mount, the
//! filesystem it shows (the device of its superblock) and the directory of that
//! filesystem it shows as its root. A directory's place is then that root followed by
//! the directory's path below the mount point. Two directories of one filesystem are
//! compared by their places: equal, or one inside the other, is refused.
//!
//! # What identifies a filesystem
//!
//! The device numbers of the mount table, which are the superblock's. They are not the
//! device number `stat` gives for a file on Btrfs: there each subvolume has a device
//! number of its own, while all of them are one filesystem with one superblock, and
//! the mount table shows that one number for every mount of every subvolume, with the
//! subvolume's path from the top of the filesystem as the mount's root. So a nested
//! subvolume mounted elsewhere is found inside its parent here, as a bind mount is.
//!
//! # What this does not see
//!
//! A filesystem stacked on another shows directories of the lower one under another
//! superblock: a data root on an overlay and a release directory that is a bind mount
//! of a directory in that overlay's upper layer are two filesystems here. The lower
//! directories of an overlay are not in the mount table of the namespace that uses it.
//!
//! # Doubt refuses
//!
//! Every way in which the answer cannot be had is a [`MountDoubt`], and the adapter
//! refuses on each: no mount id, a table that cannot be read or is not what the kernel
//! writes, a mount that is not listed or listed twice, a mount whose root was deleted,
//! a directory whose path is not below its mount's mount point.
//!
//! The GNU/Linux adapter supplies the table, the mount ids and the paths
//! (`linux/release.rs`); the decisions are compiled and tested on every target.

use super::release::{MountDoubt, RootRule};

/// One mount, as one line of the mount table describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Mount {
    /// The mount's id (the first field).
    pub(super) id: u64,
    /// The device of the filesystem the mount shows: major and minor (the third
    /// field).
    pub(super) filesystem: (u32, u32),
    /// The directory of that filesystem the mount shows as its root (the fourth
    /// field), unescaped.
    pub(super) root: Vec<u8>,
    /// Where the mount is, from this process's root (the fifth field), unescaped.
    pub(super) mount_point: Vec<u8>,
}

/// A directory's place: the filesystem it is in, and its path from the top of that
/// filesystem, as single names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Place {
    pub(super) filesystem: (u32, u32),
    pub(super) path: Vec<Vec<u8>>,
}

/// The most bytes of a mount table that are parsed.
pub(super) const LARGEST_TABLE: usize = 16 * 1024 * 1024;

/// Every mount of the mount table `table` (the bytes of `/proc/self/mountinfo`).
///
/// A line is `id parent major:minor root mount-point options [optional…] - type
/// source super-options`, its fields separated by single spaces. The kernel writes a
/// space, a tab, a newline and a backslash inside a field as a backslash and three
/// octal digits, so no field can hold a separator; a line that is not of this form is
/// [`MountDoubt::Malformed`], and so is the whole table.
pub(super) fn parse(table: &[u8]) -> Result<Vec<Mount>, MountDoubt> {
    if table.len() > LARGEST_TABLE || table.contains(&0) {
        return Err(MountDoubt::Malformed);
    }
    let mut mounts = Vec::new();
    let mut lines: Vec<&[u8]> = table.split(|byte| *byte == b'\n').collect();
    // The table ends with a newline; a last line without one was cut off.
    if lines.pop() != Some(&b""[..]) {
        return Err(MountDoubt::Malformed);
    }
    for line in lines {
        mounts.push(parse_line(line).ok_or(MountDoubt::Malformed)?);
    }
    Ok(mounts)
}

fn parse_line(line: &[u8]) -> Option<Mount> {
    let fields: Vec<&[u8]> = line.split(|byte| *byte == b' ').collect();
    // The six fixed fields, then the separator, then at least the filesystem type.
    let separator = fields.iter().position(|field| *field == b"-")?;
    if separator < 6 || fields.len() < separator + 2 || fields[separator + 1].is_empty() {
        return None;
    }
    if fields[..separator].iter().any(|field| field.is_empty()) {
        return None;
    }
    let id = number(fields[0])?;
    number(fields[1])?;
    let mut device = fields[2].split(|byte| *byte == b':');
    let filesystem = (
        u32::try_from(number(device.next()?)?).ok()?,
        u32::try_from(number(device.next()?)?).ok()?,
    );
    if device.next().is_some() {
        return None;
    }
    let root = unescape(fields[3])?;
    let mount_point = unescape(fields[4])?;
    if root.first() != Some(&b'/') || mount_point.first() != Some(&b'/') {
        return None;
    }
    Some(Mount {
        id,
        filesystem,
        root,
        mount_point,
    })
}

/// A decimal number of ASCII digits and nothing else (no sign, no space).
fn number(field: &[u8]) -> Option<u64> {
    if field.is_empty() || !field.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(field).ok()?.parse().ok()
}

/// The bytes of a field of the mount table: a backslash and three octal digits is one
/// byte. A backslash followed by anything else is not something the kernel writes.
fn unescape(field: &[u8]) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&byte, after)) = rest.split_first() {
        if byte != b'\\' {
            bytes.push(byte);
            rest = after;
            continue;
        }
        let (digits, after) = after.split_at_checked(3)?;
        let mut value = 0u32;
        for digit in digits {
            if !(b'0'..=b'7').contains(digit) {
                return None;
            }
            value = value * 8 + u32::from(digit - b'0');
        }
        bytes.push(u8::try_from(value).ok()?);
        rest = after;
    }
    Some(bytes)
}

/// The single names of the absolute path `path`, or `None` if it is not absolute or
/// has an empty name, `.` or `..` in it: no path the kernel reports has one.
fn names(path: &[u8]) -> Option<Vec<&[u8]>> {
    let below = path.strip_prefix(b"/")?;
    if below.is_empty() {
        return Some(Vec::new());
    }
    let names: Vec<&[u8]> = below.split(|byte| *byte == b'/').collect();
    if names
        .iter()
        .any(|name| name.is_empty() || *name == b"." || *name == b"..")
    {
        return None;
    }
    Some(names)
}

/// How the kernel marks a mount whose root directory was deleted, and a path whose
/// last directory was.
const DELETED_ROOT: &[u8] = b"//deleted";
const DELETED_PATH: &[u8] = b" (deleted)";

/// The one mount with the id `mount_id`, whose root directory still exists. The
/// adapter asks this before it reads a directory's path: a directory in a mount whose
/// root was deleted has no path that names it, and the reason is the mount's.
pub(super) fn listed(mounts: &[Mount], mount_id: u64) -> Result<&Mount, MountDoubt> {
    let mut listed = mounts.iter().filter(|mount| mount.id == mount_id);
    let mount = listed.next().ok_or(MountDoubt::NotListed)?;
    if listed.next().is_some() {
        return Err(MountDoubt::Ambiguous);
    }
    if mount.root.ends_with(DELETED_ROOT) || mount.root.ends_with(DELETED_PATH) {
        return Err(MountDoubt::DeletedRoot);
    }
    Ok(mount)
}

/// The place of the directory at `path` (from this process's root) that is in the
/// mount `mount_id`: the mount's root, then the path below the mount point.
pub(super) fn place(mounts: &[Mount], mount_id: u64, path: &[u8]) -> Result<Place, MountDoubt> {
    let mount = listed(mounts, mount_id)?;
    let root = names(&mount.root).ok_or(MountDoubt::Malformed)?;
    let mount_point = names(&mount.mount_point).ok_or(MountDoubt::Malformed)?;
    // A path that is not below the mount point is one this process cannot reach
    // through that mount (the kernel then writes no absolute path at all), or one of
    // a directory that was moved or deleted since.
    let path = names(path).ok_or(MountDoubt::PathOutsideMount)?;
    let below = path
        .strip_prefix(mount_point.as_slice())
        .ok_or(MountDoubt::PathOutsideMount)?;
    Ok(Place {
        filesystem: mount.filesystem,
        path: root.iter().chain(below).map(|name| name.to_vec()).collect(),
    })
}

/// The rule that keeps the release directory and the data root apart inside one
/// filesystem: their places are not equal, and neither is inside the other. Two
/// filesystems share no directory.
pub(super) fn disjoint(store: &Place, data_root: &Place) -> Result<(), RootRule> {
    if store.filesystem != data_root.filesystem {
        return Ok(());
    }
    if store.path == data_root.path {
        return Err(RootRule::IsDataRoot);
    }
    if store.path.starts_with(&data_root.path) {
        return Err(RootRule::InsideDataRoot);
    }
    if data_root.path.starts_with(&store.path) {
        return Err(RootRule::ContainsDataRoot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mount table as the kernel writes one: the root filesystem, `/proc`, a
    /// separate filesystem for `/srv`, and whatever a test adds.
    fn table(more: &[&str]) -> Vec<Mount> {
        let mut text = String::from(
            "22 27 0:21 / /proc rw,relatime - proc proc rw\n\
             27 1 254:0 / / rw,relatime - ext4 /dev/vda rw,discard\n\
             40 27 254:16 / /srv rw,relatime shared:5 master:2 - ext4 /dev/vdb rw\n",
        );
        for line in more {
            text.push_str(line);
            text.push('\n');
        }
        parse(text.as_bytes()).unwrap()
    }

    fn at(mounts: &[Mount], mount_id: u64, path: &str) -> Result<Place, MountDoubt> {
        place(mounts, mount_id, path.as_bytes())
    }

    fn judged(
        mounts: &[Mount],
        store: (u64, &str),
        data_root: (u64, &str),
    ) -> Result<(), RootRule> {
        disjoint(
            &at(mounts, store.0, store.1).unwrap(),
            &at(mounts, data_root.0, data_root.1).unwrap(),
        )
    }

    #[test]
    fn a_line_of_the_mount_table_is_read_exactly() {
        let mounts = table(&[
            // Optional fields, none and several; an empty source.
            "61 27 254:0 /home/a/agentic /data rw,relatime - ext4 /dev/vda rw",
            "62 27 254:0 /home/a/agentic/secrets /secrets rw shared:1 master:2 propagate_from:3 - ext4 /dev/vda rw",
            "63 27 0:44 / /run/x rw - tmpfs  rw",
            // A space, a tab, a newline and a backslash in a path, as the kernel writes them.
            r"64 27 254:0 /with\040space/and\011tab /mnt/new\012line\134slash rw - ext4 /dev/vda rw",
        ]);
        assert_eq!(mounts.len(), 7);
        assert_eq!(
            mounts[1],
            Mount {
                id: 27,
                filesystem: (254, 0),
                root: b"/".to_vec(),
                mount_point: b"/".to_vec(),
            }
        );
        assert_eq!(mounts[2].filesystem, (254, 16));
        assert_eq!(mounts[3].root, b"/home/a/agentic");
        assert_eq!(mounts[4].mount_point, b"/secrets");
        assert_eq!(mounts[5].filesystem, (0, 44));
        assert_eq!(mounts[6].root, b"/with space/and\ttab");
        assert_eq!(mounts[6].mount_point, b"/mnt/new\nline\\slash");
        // An empty table is a table without a mount; an empty line is no line of one.
        assert_eq!(parse(b""), Ok(Vec::new()));
        assert_eq!(parse(b"\n"), Err(MountDoubt::Malformed));
    }

    #[test]
    fn a_table_that_is_not_what_the_kernel_writes_is_refused_whole() {
        let good = "27 1 254:0 / / rw,relatime - ext4 /dev/vda rw\n";
        assert!(parse(good.as_bytes()).is_ok());
        for hostile in [
            // Cut off: no newline at the end, or in the middle of a line.
            "27 1 254:0 / / rw,relatime - ext4 /dev/vda rw",
            "27 1 254:0 / /",
            // No separator, or the separator too early, or nothing after it.
            "27 1 254:0 / / rw,relatime ext4 /dev/vda rw\n",
            "27 1 254:0 / / - ext4 /dev/vda rw\n",
            "27 1 254:0 / / rw,relatime -\n",
            "27 1 254:0 / / rw,relatime - \n",
            // Fields that are not numbers, or more than numbers.
            "x 1 254:0 / / rw - ext4 /dev/vda rw\n",
            "27 1 254 / / rw - ext4 /dev/vda rw\n",
            "27 1 254:0:1 / / rw - ext4 /dev/vda rw\n",
            "27 1 254:x / / rw - ext4 /dev/vda rw\n",
            "+27 1 254:0 / / rw - ext4 /dev/vda rw\n",
            "27 -1 254:0 / / rw - ext4 /dev/vda rw\n",
            "99999999999999999999999 1 254:0 / / rw - ext4 /dev/vda rw\n",
            "27 1 4294967296:0 / / rw - ext4 /dev/vda rw\n",
            // Paths that are not absolute.
            "27 1 254:0 data / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 / data rw - ext4 /dev/vda rw\n",
            // Two spaces: an empty field among the fixed ones.
            "27 1 254:0  / / rw - ext4 /dev/vda rw\n",
            "27  1 254:0 / / rw - ext4 /dev/vda rw\n",
            // Escapes the kernel does not write: too short, not octal, too large, bare.
            "27 1 254:0 /a\\04 / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 /a\\ / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 /a\\089 / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 /a\\777 / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 /a\\\\ / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 / /a\\x41 rw - ext4 /dev/vda rw\n",
            // A tab where a space belongs; a NUL byte; an empty line in the middle.
            "27 1 254:0\t/ / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 /a\0b / rw - ext4 /dev/vda rw\n",
            "27 1 254:0 / / rw - ext4 /dev/vda rw\n\n22 27 0:21 / /proc rw - proc proc rw\n",
        ] {
            assert_eq!(
                parse(hostile.as_bytes()),
                Err(MountDoubt::Malformed),
                "{hostile:?}"
            );
            // One such line spoils a table of good ones.
            let mixed = format!("{good}{hostile}");
            if hostile.ends_with('\n') {
                assert_eq!(parse(mixed.as_bytes()), Err(MountDoubt::Malformed));
            }
        }
        // A path cannot fake a separator or a second line: the kernel escapes both.
        let escaped =
            parse(b"27 1 254:0 /a\\040-\\040ext4 /b\\01228\\0401 rw - ext4 /dev/vda rw\n");
        let escaped = escaped.unwrap();
        assert_eq!(escaped.len(), 1);
        assert_eq!(escaped[0].root, b"/a - ext4");
        assert_eq!(escaped[0].mount_point, b"/b\n28 1");
        let mut oversized = good.as_bytes().to_vec();
        oversized.resize(LARGEST_TABLE + 1, b'x');
        assert_eq!(parse(&oversized), Err(MountDoubt::Malformed));
    }

    #[test]
    fn a_place_is_the_mounts_root_and_the_path_below_the_mount_point() {
        let mounts = table(&[
            "61 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
            r"64 27 254:0 /with\040space /mnt/new\012line rw - ext4 /dev/vda rw",
        ]);
        let names = |place: Place| -> Vec<String> {
            place
                .path
                .iter()
                .map(|name| String::from_utf8_lossy(name).into_owned())
                .collect()
        };
        assert_eq!(names(at(&mounts, 27, "/").unwrap()), [""; 0]);
        assert_eq!(
            names(at(&mounts, 27, "/home/a/x").unwrap()),
            ["home", "a", "x"]
        );
        assert_eq!(
            names(at(&mounts, 61, "/data").unwrap()),
            ["home", "a", "agentic"]
        );
        assert_eq!(
            names(at(&mounts, 61, "/data/office-state/offices").unwrap()),
            ["home", "a", "agentic", "office-state", "offices"]
        );
        assert_eq!(names(at(&mounts, 40, "/srv/x").unwrap()), ["x"]);
        assert_eq!(at(&mounts, 40, "/srv/x").unwrap().filesystem, (254, 16));
        assert_eq!(
            names(at(&mounts, 64, "/mnt/new\nline/below it").unwrap()),
            ["with space", "below it"]
        );
        // The mount point is a prefix by whole names only.
        assert_eq!(
            at(&mounts, 61, "/database"),
            Err(MountDoubt::PathOutsideMount)
        );
    }

    #[test]
    fn every_doubt_about_a_place_is_reported_and_none_is_a_place() {
        let mounts = table(&[
            "61 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
            // The same id twice.
            "70 27 254:0 /x /one rw - ext4 /dev/vda rw",
            "70 27 254:0 /y /two rw - ext4 /dev/vda rw",
            // Mounts whose root directory was deleted, as the kernel marks them.
            "71 27 254:0 /home/a/gone//deleted /gone rw - ext4 /dev/vda rw",
            r"72 27 254:0 /home/a/gone\040(deleted) /gone2 rw - ext4 /dev/vda rw",
            // Paths the kernel does not write.
            "73 27 254:0 /a/../b /dots rw - ext4 /dev/vda rw",
            "74 27 254:0 /a /dots/./x rw - ext4 /dev/vda rw",
            "75 27 254:0 /a//b /slashes rw - ext4 /dev/vda rw",
        ]);
        assert_eq!(at(&mounts, 99, "/data"), Err(MountDoubt::NotListed));
        assert_eq!(at(&mounts, 70, "/one"), Err(MountDoubt::Ambiguous));
        assert_eq!(at(&mounts, 71, "/gone"), Err(MountDoubt::DeletedRoot));
        assert_eq!(at(&mounts, 72, "/gone2"), Err(MountDoubt::DeletedRoot));
        assert_eq!(at(&mounts, 73, "/dots"), Err(MountDoubt::Malformed));
        assert_eq!(at(&mounts, 74, "/dots/./x"), Err(MountDoubt::Malformed));
        assert_eq!(at(&mounts, 75, "/slashes"), Err(MountDoubt::Malformed));
        // The directory's own path: not absolute (the kernel's word for a directory this
        // process cannot reach), not below its mount, or not a path at all.
        for path in [
            "",
            "data",
            "(unreachable)/data",
            "/",
            "/home/a/agentic",
            "/srv",
            "/data/../data",
            "/data//x",
            "/data/",
        ] {
            assert_eq!(
                at(&mounts, 61, path),
                Err(MountDoubt::PathOutsideMount),
                "{path:?}"
            );
        }
        assert_eq!(at(&[], 27, "/"), Err(MountDoubt::NotListed));
    }

    /// The reviewer's case and its reverse: one of the two is a bind mount of a
    /// directory inside the other. The walk through `..` cannot see it; the places do.
    #[test]
    fn a_bind_mount_of_a_directory_inside_the_other_is_found_by_its_place() {
        let mounts = table(&[
            // `mount --bind data/inner alias2`, both below `/work` on the root filesystem.
            "80 27 254:0 /work/data/inner /work/alias2 rw - ext4 /dev/vda rw",
            // `docker run -v ./agentic:/data -v ./agentic/secrets:/secrets`.
            "81 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
            "82 27 254:0 /home/a/agentic/secrets /secrets rw - ext4 /dev/vda rw",
        ]);
        // The store is a bind of a directory of the data root.
        assert_eq!(
            judged(&mounts, (80, "/work/alias2"), (27, "/work/data")),
            Err(RootRule::InsideDataRoot)
        );
        assert_eq!(
            judged(&mounts, (82, "/secrets"), (81, "/data")),
            Err(RootRule::InsideDataRoot)
        );
        // A directory below such a bind is inside the data root as well.
        assert_eq!(
            judged(&mounts, (80, "/work/alias2/deeper"), (27, "/work/data")),
            Err(RootRule::InsideDataRoot)
        );
        // The reverse: the data root is a bind of a directory of the store.
        assert_eq!(
            judged(&mounts, (27, "/work/data"), (80, "/work/alias2")),
            Err(RootRule::ContainsDataRoot)
        );
        assert_eq!(
            judged(&mounts, (81, "/data"), (82, "/secrets")),
            Err(RootRule::ContainsDataRoot)
        );
    }

    /// The two cases the walk through `..` already refuses are refused by their places
    /// too, so the two checks never disagree about them.
    #[test]
    fn the_same_directory_through_a_bind_and_a_store_below_a_bind_alias_are_refused() {
        let mounts = table(&[
            "83 27 254:0 /work/store /work/store-again rw - ext4 /dev/vda rw",
            "84 27 254:0 /work/data /work/data-alias rw - ext4 /dev/vda rw",
        ]);
        // The same directory twice, once through a bind.
        assert_eq!(
            judged(&mounts, (27, "/work/store"), (83, "/work/store-again")),
            Err(RootRule::IsDataRoot)
        );
        assert_eq!(
            judged(
                &mounts,
                (83, "/work/store-again"),
                (83, "/work/store-again")
            ),
            Err(RootRule::IsDataRoot)
        );
        // A store inside a data root that was given through a bind alias.
        assert_eq!(
            judged(
                &mounts,
                (27, "/work/data/credentials"),
                (84, "/work/data-alias")
            ),
            Err(RootRule::InsideDataRoot)
        );
        assert_eq!(
            judged(
                &mounts,
                (84, "/work/data-alias/credentials"),
                (27, "/work/data")
            ),
            Err(RootRule::InsideDataRoot)
        );
    }

    #[test]
    fn directories_that_share_nothing_are_accepted() {
        let mounts = table(&[
            // Two named volumes of one disk, as a container sees them.
            "85 27 254:0 /var/lib/docker/volumes/data/_data /data rw - ext4 /dev/vda rw",
            "86 27 254:0 /var/lib/docker/volumes/secrets/_data /secrets rw - ext4 /dev/vda rw",
            // A name that only starts like the other.
            "87 27 254:0 /work/data-2 /elsewhere rw - ext4 /dev/vda rw",
        ]);
        assert_eq!(judged(&mounts, (86, "/secrets"), (85, "/data")), Ok(()));
        assert_eq!(
            judged(
                &mounts,
                (27, "/home/a/.local/state/x"),
                (27, "/home/a/.local/share/x")
            ),
            Ok(())
        );
        assert_eq!(
            judged(&mounts, (87, "/elsewhere"), (27, "/work/data")),
            Ok(())
        );
        assert_eq!(
            judged(&mounts, (27, "/work/data"), (87, "/elsewhere")),
            Ok(())
        );
        // Another filesystem shares no directory, whatever the paths are: the same
        // path on two devices, and a filesystem mounted below the data root. (The
        // second is refused all the same, by the walk through `..`: `rules::disjoint`.)
        assert_eq!(judged(&mounts, (40, "/srv"), (27, "/")), Ok(()));
        let below = table(&["88 27 254:32 / /work/data/secrets rw - ext4 /dev/vdc rw"]);
        assert_eq!(
            judged(&below, (88, "/work/data/secrets"), (27, "/work/data")),
            Ok(())
        );
        assert_eq!(
            crate::credentials::rules::disjoint(
                // (device, inode) chains: the mount's root, then the data root, `/work`, `/`.
                &[
                    ((254 << 8) | 32, 2),
                    (0xfe00, 20),
                    (0xfe00, 10),
                    (0xfe00, 2)
                ],
                &[(0xfe00, 20), (0xfe00, 10), (0xfe00, 2)],
            ),
            Err(RootRule::InsideDataRoot)
        );
    }

    /// Btrfs: every subvolume has a device number of its own in `stat`, and all of
    /// them are one filesystem in the mount table, where a subvolume's mount names the
    /// subvolume's path as its root. A nested subvolume mounted elsewhere is inside
    /// its parent, as a bind mount is.
    #[test]
    fn subvolumes_of_one_filesystem_are_compared_by_their_paths_in_it() {
        let mounts = parse(
            b"27 1 0:30 /@ / rw - btrfs /dev/vda2 rw,subvol=/@\n\
              90 27 0:30 /@data /data rw - btrfs /dev/vda2 rw,subvol=/@data\n\
              91 27 0:30 /@data/secrets /secrets rw - btrfs /dev/vda2 rw,subvol=/@data/secrets\n\
              92 27 0:30 /@secrets /secrets2 rw - btrfs /dev/vda2 rw,subvol=/@secrets\n",
        )
        .unwrap();
        assert_eq!(
            judged(&mounts, (91, "/secrets"), (90, "/data")),
            Err(RootRule::InsideDataRoot)
        );
        assert_eq!(
            judged(&mounts, (90, "/data"), (91, "/secrets")),
            Err(RootRule::ContainsDataRoot)
        );
        assert_eq!(judged(&mounts, (92, "/secrets2"), (90, "/data")), Ok(()));
        assert_eq!(judged(&mounts, (92, "/secrets2"), (27, "/home/a")), Ok(()));
    }
}
