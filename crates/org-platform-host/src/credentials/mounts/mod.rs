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
//! # Doubt about the two mounts refuses; every other line is not read
//!
//! The two directories are found in the table by the mount ids the kernel gives for
//! their descriptors. Every way in which the place of one of *them* cannot be had is
//! a [`MountDoubt`], and the adapter refuses on each: no mount id, a table that cannot
//! be read, a mount that is not listed or listed twice, a line for it that is not
//! what the kernel writes, a root that is no path or was deleted, a directory whose
//! path is not below its mount's mount point.
//!
//! Of every other line only the mount id is read, so a machine's other mounts,
//! whatever they are (Docker's network namespaces, a FUSE mount with spaces in its
//! source, an overlay with a thousand bytes of options), cannot refuse a store. A
//! line whose id cannot be read does: it could be one of the two. [`Table`] says why
//! that is all that has to be read.
//!
//! A mount over another at the same mount point does not matter either: a directory
//! is placed by its own mount id, not by which mount a path would reach today.
//!
//! One question reads one more field of every line: whether anything is mounted inside
//! the release directory ([`Table::mounted_inside`]). That is about every mount, so the
//! mount point of each line is read, and a line whose mount point cannot be read refuses.
//! The kernel writes a mount point as an absolute path, escaped, before any field of
//! free text, so a machine's other mounts do not make that read fail.
//!
//! One more question reads three fields of every line: whether another mount shows the
//! release directory somewhere else ([`Table::aliases`]): the same volume mounted twice,
//! a folder of it mounted elsewhere, or a mount of a directory that contains it. That is
//! about every mount too, so the device, the root and the mount point of each line are
//! read, and a line where they cannot be read refuses. All three come before any field of
//! free text. A line of another filesystem, a mount of a namespace file (its root is no
//! path) and a mount whose root was deleted show no directory of the store.
//!
//! The GNU/Linux adapter supplies the table, the mount ids and the paths
//! (`linux/release/`); the decisions are compiled and tested on every target.

use super::release::{MountDoubt, RootRule};

/// One mount that a directory is in, as its line of the mount table describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Mount {
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

/// The most bytes of a mount table that are read. A node with ten thousand container
/// mounts has a table of a few megabytes.
pub(super) const LARGEST_TABLE: usize = 64 * 1024 * 1024;

/// Another mount through which the release directory, or a part of it, can be reached
/// ([`Table::aliases`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Alias {
    /// Where the other mount is, from this process's root, unescaped.
    pub(super) mount_point: Vec<u8>,
    /// Where the store, or the part of it the mount shows, is reached through it, from
    /// this process's root, as single names.
    pub(super) shows: Vec<Vec<u8>>,
}

impl Alias {
    /// Whether what the mount shows of the store is at or below the directory at `path`
    /// (from this process's root). A path that is not absolute is
    /// [`MountDoubt::Malformed`].
    pub(super) fn within(&self, path: &[u8]) -> Result<bool, MountDoubt> {
        let directory = names(path).ok_or(MountDoubt::Malformed)?;
        Ok(starts_with(&self.shows, &directory))
    }
}

/// Whether the names `path` begin with the names `prefix` (or are equal to them).
fn starts_with<A: AsRef<[u8]>, B: AsRef<[u8]>>(path: &[A], prefix: &[B]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(name, other)| name.as_ref() == other.as_ref())
}

/// The mount table, indexed: every line with the mount id it starts with.
///
/// # What is read of a line, and why that is enough
///
/// A line is `id parent major:minor root mount-point options [optional…] - type
/// source super-options`, its fields separated by single spaces. The kernel writes a
/// space, a tab, a newline and a backslash inside a field as a backslash and three
/// octal digits, so no field can hold a separator and no line can hold a second one.
///
/// The two directories the check is about are found in this table **by the mount id
/// the kernel gives for their held descriptors** (`statx`), never by a path. So:
///
/// - Of **every** line only the mount id is read (the first field: decimal digits up
///   to the first space). Nothing else of a line that is not one of the two mounts
///   looked for is read, so nothing else of it can refuse a store: a mount of a
///   namespace file whose root is `net:[4026532268]`, a mount whose source has
///   spaces, options of any length, a type this code has never heard of.
/// - A line whose mount id **cannot be read** refuses the whole table
///   ([`MountDoubt::Malformed`]): that line could be one of the two mounts looked
///   for, or a second line for one of them, and it cannot be told. The same holds
///   for a table that does not end with a newline: its last line was cut off, and a
///   cut-off number is another number.
/// - The one line with a mount id that is looked for is read in full and has to be a
///   mount of a directory ([`Table::mount`]); anything else about it is a doubt.
///
/// Reading so little cannot hide a store inside the data root. What decides is the
/// place of each of the two directories, and a place is made of that directory's own
/// line and its own path and of nothing else in the table. A line that is skipped is
/// the line of another mount, and another mount's line says nothing about where
/// these two directories are. The only way another line could matter is by carrying
/// one of the two ids, and every line's id is read.
#[derive(Debug)]
pub(super) struct Table<'a> {
    lines: Vec<(u64, &'a [u8])>,
}

impl<'a> Table<'a> {
    /// The index of the mount table `table` (the bytes of `/proc/self/mountinfo`).
    pub(super) fn of(table: &'a [u8]) -> Result<Self, MountDoubt> {
        if table.len() > LARGEST_TABLE {
            return Err(MountDoubt::Malformed);
        }
        let mut lines: Vec<&[u8]> = table.split(|byte| *byte == b'\n').collect();
        // The table ends with a newline; a last line without one was cut off.
        if lines.pop() != Some(&b""[..]) {
            return Err(MountDoubt::Malformed);
        }
        lines
            .into_iter()
            .map(|line| {
                let id = line.split(|byte| *byte == b' ').next().and_then(number);
                id.map(|id| (id, line)).ok_or(MountDoubt::Malformed)
            })
            .collect::<Result<_, _>>()
            .map(|lines| Self { lines })
    }

    /// The first mount whose mount point is inside the directory at `path` (from this
    /// process's root), not the directory itself: its mount point. A mount there puts
    /// another volume, or another directory of this one, under the directory's own names,
    /// so whatever is written below it is not in the directory's volume.
    ///
    /// Here the mount point of **every** line is read (the fifth field), since any line
    /// could be such a mount. A line whose mount point cannot be read is
    /// [`MountDoubt::Malformed`]: it could be one. Nothing else of those lines is read.
    pub(super) fn mounted_inside(&self, path: &[u8]) -> Result<Option<Vec<u8>>, MountDoubt> {
        let directory = names(path).ok_or(MountDoubt::Malformed)?;
        for (_, line) in &self.lines {
            let field = line
                .split(|byte| *byte == b' ')
                .nth(4)
                .ok_or(MountDoubt::Malformed)?;
            let mount_point = unescape(field).ok_or(MountDoubt::Malformed)?;
            let point = names(&mount_point).ok_or(MountDoubt::Malformed)?;
            if point.len() > directory.len() && point.starts_with(&directory) {
                return Ok(Some(mount_point));
            }
        }
        Ok(None)
    }

    /// Every mount, other than `own` (the mount the release directory is in) and any
    /// mount at its mount point, that shows a directory of the release directory's
    /// filesystem at `store` (its place), inside it, or containing it: where else the
    /// store, or a part of it, can be reached, and through which mount.
    ///
    /// A mount at the same mount point as `own` is not one: one of the two hides the
    /// other, and `own` is the one the release directory was reached through.
    ///
    /// Here the device, the root and the mount point of **every** line are read (the
    /// third to fifth fields), since any line could be such a mount. A line where one of
    /// them cannot be read is [`MountDoubt::Malformed`]: it could be one. A line of
    /// another filesystem is not read past its device, and one whose root is no path (a
    /// namespace file) or was deleted is not read past its root: neither shows a
    /// directory through which anything of the store can be reached.
    pub(super) fn aliases(&self, own: &Mount, store: &Place) -> Result<Vec<Alias>, MountDoubt> {
        let mut found = Vec::new();
        for (_, line) in &self.lines {
            let mut fields = line.split(|byte| *byte == b' ').skip(2);
            let (Some(filesystem), Some(root), Some(mount_point)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(MountDoubt::Malformed);
            };
            if device(filesystem).ok_or(MountDoubt::Malformed)? != store.filesystem {
                continue;
            }
            let root = unescape(root).ok_or(MountDoubt::Malformed)?;
            // Only the kernel's own mark of a deleted root (`//deleted`) is passed over: a
            // folder may be named `… (deleted)`, and a mount of it still shows the store.
            if root.first() != Some(&b'/') || root.ends_with(DELETED_ROOT) {
                continue;
            }
            let mount_point = unescape(mount_point).ok_or(MountDoubt::Malformed)?;
            if mount_point == own.mount_point {
                continue;
            }
            let root = names(&root).ok_or(MountDoubt::Malformed)?;
            let at = names(&mount_point).ok_or(MountDoubt::Malformed)?;
            let shows: Vec<Vec<u8>> = if starts_with(&root, &store.path) {
                // A folder of the store, or the store itself: it is at the mount point.
                at.iter().map(|name| name.to_vec()).collect()
            } else if starts_with(&store.path, &root) {
                // A directory that contains the store: it is below the mount point.
                let below = &store.path[root.len()..];
                at.iter()
                    .map(|name| name.to_vec())
                    .chain(below.iter().cloned())
                    .collect()
            } else {
                continue;
            };
            found.push(Alias { mount_point, shows });
        }
        Ok(found)
    }

    /// How many mounts the table lists.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.lines.len()
    }

    /// The one mount with the id `mount_id`, read in full: a mount that shows a
    /// directory of a filesystem which still exists.
    ///
    /// - No line with that id: [`MountDoubt::NotListed`]. More than one:
    ///   [`MountDoubt::Ambiguous`].
    /// - A line that is not of the form above (a missing field or separator, a device
    ///   that is not two numbers, an escape the kernel does not write, a mount point
    ///   that is no absolute path): [`MountDoubt::Malformed`].
    /// - A root that is not an absolute path (`net:[4026532268]`: the mount of a
    ///   namespace file, which shows no directory): [`MountDoubt::RootNotAPath`]. No
    ///   directory is in such a mount, so a directory that claims to be is in doubt.
    /// - A root the kernel marks as deleted: [`MountDoubt::DeletedRoot`].
    ///
    /// A mount whose root is a file (a container's `/etc/hosts` is a bind of one) is
    /// a line like any other; no directory's mount id is its id.
    pub(super) fn mount(&self, mount_id: u64) -> Result<Mount, MountDoubt> {
        let mut listed = self.lines.iter().filter(|(id, _)| *id == mount_id);
        let (_, line) = listed.next().ok_or(MountDoubt::NotListed)?;
        if listed.next().is_some() {
            return Err(MountDoubt::Ambiguous);
        }
        let mount = fixed_fields(line).ok_or(MountDoubt::Malformed)?;
        if mount.root.first() != Some(&b'/') {
            return Err(MountDoubt::RootNotAPath);
        }
        if mount.root.ends_with(DELETED_ROOT) || mount.root.ends_with(DELETED_PATH) {
            return Err(MountDoubt::DeletedRoot);
        }
        Ok(mount)
    }
}

/// The fields of a line that a place is made of: the device, the root and the mount
/// point, the last two unescaped. `None` if the line is not of the kernel's form. The
/// root is whatever the kernel wrote there; the caller requires a path.
fn fixed_fields(line: &[u8]) -> Option<Mount> {
    if line.contains(&0) {
        return None;
    }
    let fields: Vec<&[u8]> = line.split(|byte| *byte == b' ').collect();
    // The six fixed fields, then the separator, then at least the filesystem type.
    // What follows the type (the source, the superblock's options) is not read: a
    // source may be empty.
    let separator = fields.iter().position(|field| *field == b"-")?;
    if separator < 6 || fields.len() < separator + 2 || fields[separator + 1].is_empty() {
        return None;
    }
    if fields[..separator].iter().any(|field| field.is_empty()) {
        return None;
    }
    number(fields[0])?;
    number(fields[1])?;
    let filesystem = device(fields[2])?;
    let root = unescape(fields[3])?;
    let mount_point = unescape(fields[4])?;
    if mount_point.first() != Some(&b'/') {
        return None;
    }
    Some(Mount {
        filesystem,
        root,
        mount_point,
    })
}

/// The device field of a line, `major:minor`, as two numbers.
fn device(field: &[u8]) -> Option<(u32, u32)> {
    let mut numbers = field.split(|byte| *byte == b':');
    let filesystem = (
        u32::try_from(number(numbers.next()?)?).ok()?,
        u32::try_from(number(numbers.next()?)?).ok()?,
    );
    if numbers.next().is_some() {
        return None;
    }
    Some(filesystem)
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

/// The place of the directory at `path` (from this process's root) that is in the
/// mount `mount`: the mount's root, then the path below the mount point.
pub(super) fn place(mount: &Mount, path: &[u8]) -> Result<Place, MountDoubt> {
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

/// The place of what the mount `mount` shows: its root, the top of the subtree of its
/// filesystem that is visible through it. For a volume mounted into a container that
/// is the volume's own directory.
pub(super) fn shown(mount: &Mount) -> Result<Place, MountDoubt> {
    let root = names(&mount.root).ok_or(MountDoubt::Malformed)?;
    Ok(Place {
        filesystem: mount.filesystem,
        path: root.iter().map(|name| name.to_vec()).collect(),
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

/// The rule that keeps the secrets volume and the data volume apart, for a store that
/// must be a mount point (a container): the release directory is not inside the
/// directory that the data root's mount shows (`data_volume`, [`shown`]), is not that
/// directory, and does not contain it.
///
/// [`disjoint`] compares the store with the data root itself, and a data root is not
/// always the top of its volume. In the container the data volume is mounted at
/// `/data` and the instance's root is `/data/instance`; with `-v D:/data -v
/// D/secrets:/secrets` the store is beside the instance root, not inside it, and yet
/// every key is in the data volume. Compared with what the data root's mount shows,
/// it is inside.
///
/// This is asked only where a mount point is required. On an ordinary machine the
/// data root's mount is the root filesystem's, which shows everything on it: a store
/// on the same disk is always inside that, and is meant to be.
pub(super) fn separate_volumes(store: &Place, data_volume: &Place) -> Result<(), RootRule> {
    if store.filesystem != data_volume.filesystem {
        return Ok(());
    }
    if store.path.starts_with(&data_volume.path) {
        return Err(RootRule::InsideDataVolume);
    }
    if data_volume.path.starts_with(&store.path) {
        return Err(RootRule::ContainsDataVolume);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
