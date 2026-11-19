//! dm-crypt (LUKS) under the filesystem that holds the path, read without privilege.
//!
//! The folder's device number (`stat`) names a block device, or, for a filesystem that is
//! not one block device's (Btrfs, overlayfs, tmpfs, a network or user-space filesystem), an
//! anonymous one. A block device is followed through `/sys/dev/block/<major>:<minor>`: a
//! device-mapper target whose UUID names an encrypting dm-crypt format ([`ENCRYPTING`]:
//! LUKS1, LUKS2, plain, TrueCrypt, BitLocker, FileVault2) is `On`; every other
//! device-mapper target (LVM, multipath, and dm-integrity and dm-verity, whose UUIDs also
//! begin `CRYPT-` and which encrypt nothing) and a software RAID are as encrypted as the
//! devices in their `slaves`; a partition is as encrypted as the device it is a partition
//! of; a plain disk is `Off`. An anonymous device is looked up in `/proc/self/mountinfo`:
//! Btrfs names one of its block devices there, the volume it belongs to is found in
//! `/sys/fs/btrfs`, and the state is `On` only if every device of the volume is; every
//! other filesystem is `Unknown`, with its type as the reason.
//!
//! What this cannot see, and therefore reports as `Off` on a plain disk: encryption by the
//! drive itself, by a hypervisor or a cloud provider, and fscrypt on a folder. The wording
//! says so. Nothing is guessed: a step that cannot be read is `Unknown` with the step.
use super::{DiskEncryption, EncryptionState, Wording};
use crate::Refused;
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

pub(super) const WORDING: Wording = Wording {
    name: "LUKS/dm-crypt",
    setting: "LUKS for the disk that holds this folder (encryption by the drive itself or \
              by a cloud provider is not visible from here)",
};

const SYS_BLOCK: &str = "/sys/dev/block";
const SYS_BTRFS: &str = "/sys/fs/btrfs";
const MOUNTINFO: &str = "/proc/self/mountinfo";
/// The device-mapper UUID prefixes of the dm-crypt formats that encrypt (cryptsetup's own
/// spellings). `CRYPT-INTEGRITY-` and `CRYPT-VERITY-` are not among them: they add
/// checksums and encrypt nothing.
const ENCRYPTING: [&str; 6] = [
    "CRYPT-LUKS1-",
    "CRYPT-LUKS2-",
    "CRYPT-PLAIN-",
    "CRYPT-TCRYPT-",
    "CRYPT-BITLK-",
    "CRYPT-FVAULT2-",
];
/// How many device layers are followed (a partition of a RAID of LVM of LUKS is four).
const MAX_DEPTH: u8 = 8;

pub(super) fn disk_encryption() -> Result<Box<dyn DiskEncryption>, Refused> {
    Ok(Box::new(DmCrypt {
        sys: SYS_BLOCK.into(),
        btrfs: SYS_BTRFS.into(),
        mountinfo: MOUNTINFO.into(),
    }))
}

struct DmCrypt {
    sys: PathBuf,
    btrfs: PathBuf,
    mountinfo: PathBuf,
}

impl DiskEncryption for DmCrypt {
    fn state(&self, path: &Path) -> EncryptionState {
        let Some(target) = nearest_existing(path) else {
            return EncryptionState::Unknown(format!("no part of {} exists", path.display()));
        };
        let device = match fs::metadata(&target) {
            Ok(meta) => meta.dev(),
            Err(e) => {
                return EncryptionState::Unknown(format!("cannot read {}: {e}", target.display()))
            }
        };
        let (major, minor) = (libc::major(device), libc::minor(device));
        if major != 0 {
            return layer(&self.sys, (major, minor), 0);
        }
        // An anonymous device: the filesystem is not one block device's.
        let table = match fs::read_to_string(&self.mountinfo) {
            Ok(table) => table,
            Err(e) => {
                return EncryptionState::Unknown(format!(
                    "cannot read {}: {e}",
                    self.mountinfo.display()
                ))
            }
        };
        match mounted(&table, (major, minor)) {
            Some((kind, source)) if kind == "btrfs" => match block_device(Path::new(&source)) {
                Ok(device) => btrfs_volume(&self.sys, &self.btrfs, device),
                Err(why) => EncryptionState::Unknown(why),
            },
            Some((kind, _)) => EncryptionState::Unknown(format!(
                "the folder is on {kind}, which is not a single block device"
            )),
            None => EncryptionState::Unknown(format!(
                "the folder's filesystem ({major}:{minor}) is not in the mount table"
            )),
        }
    }
}

fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|p| !p.as_os_str().is_empty() && p.symlink_metadata().is_ok())
        .map(Path::to_path_buf)
}

/// The filesystem type and source of the mount whose device number is `device`, from the
/// text of `/proc/self/mountinfo` (`proc(5)`): `id parent major:minor root mount-point
/// options [optional...] - type source super-options`.
fn mounted(table: &str, (major, minor): (u32, u32)) -> Option<(String, String)> {
    let wanted = format!("{major}:{minor}");
    table.lines().find_map(|line| {
        let (before, after) = line.split_once(" - ")?;
        if before.split(' ').nth(2)? != wanted {
            return None;
        }
        let mut after = after.split(' ');
        Some((after.next()?.to_owned(), unescape(after.next()?)))
    })
}

/// The mount table writes space, tab, line feed and backslash as `\ooo`.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let octal = bytes.get(at + 1..at + 4).filter(|_| bytes[at] == b'\\');
        match octal.and_then(|o| u8::from_str_radix(std::str::from_utf8(o).ok()?, 8).ok()) {
            Some(byte) => {
                out.push(byte);
                at += 4;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The device number of the block device at `path`.
fn block_device(path: &Path) -> Result<(u32, u32), String> {
    let meta = fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !meta.file_type().is_block_device() {
        return Err(format!("{} is not a block device", path.display()));
    }
    Ok((libc::major(meta.rdev()), libc::minor(meta.rdev())))
}

/// `major:minor` from a sysfs `dev` file.
fn device_number(file: &Path) -> Option<(u32, u32)> {
    let text = fs::read_to_string(file).ok()?;
    let (major, minor) = text.trim().split_once(':')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The state of a Btrfs volume, found by one of its devices: `On` only if every device
/// the kernel lists for the volume is, `Off` if one certainly is not, and `Unknown` when
/// the volume or one of its devices cannot be read.
fn btrfs_volume(sys: &Path, btrfs: &Path, member: (u32, u32)) -> EncryptionState {
    let unknown = |why: String| EncryptionState::Unknown(why);
    let Ok(volumes) = fs::read_dir(btrfs) else {
        return unknown(format!(
            "the devices of the Btrfs volume cannot be read from {}",
            btrfs.display()
        ));
    };
    for volume in volumes.flatten() {
        let Ok(entries) = fs::read_dir(volume.path().join("devices")) else {
            continue;
        };
        let devices: Vec<Option<(u32, u32)>> = entries
            .map(|entry| {
                entry
                    .ok()
                    .and_then(|e| device_number(&e.path().join("dev")))
            })
            .collect();
        if !devices.contains(&Some(member)) {
            continue;
        }
        return combine(
            devices
                .into_iter()
                .map(|device| match device {
                    Some(device) => layer(sys, device, 0),
                    None => unknown("a device of the Btrfs volume cannot be read".into()),
                })
                .collect(),
        )
        .unwrap_or_else(|| unknown("the Btrfs volume lists no device".into()));
    }
    unknown(format!(
        "the Btrfs volume of device {}:{} is not listed in {}",
        member.0,
        member.1,
        btrfs.display()
    ))
}

/// The state of several devices that hold one volume together: `Off` if any is,
/// otherwise `Unknown` if any is, otherwise `On`. `None` for no device at all.
fn combine(states: Vec<EncryptionState>) -> Option<EncryptionState> {
    if states.is_empty() {
        return None;
    }
    let mut unknown = None;
    for state in states {
        match state {
            EncryptionState::Off => return Some(EncryptionState::Off),
            EncryptionState::Unknown(why) => {
                unknown.get_or_insert(why);
            }
            EncryptionState::On => {}
        }
    }
    Some(match unknown {
        Some(why) => EncryptionState::Unknown(why),
        None => EncryptionState::On,
    })
}

/// The state of block device `device`, from its directory below `sys`. Never `On` without
/// an encrypting dm-crypt target on the way, and `Off` only when the chain ends at a plain
/// disk; whatever cannot be followed is `Unknown`.
fn layer(sys: &Path, (major, minor): (u32, u32), depth: u8) -> EncryptionState {
    if depth > MAX_DEPTH {
        return EncryptionState::Unknown(
            "the block devices under the folder nest too deeply".into(),
        );
    }
    let dir = sys.join(format!("{major}:{minor}"));
    if !dir.is_dir() {
        return EncryptionState::Unknown(format!(
            "block device {major}:{minor} is not listed in {}",
            sys.display()
        ));
    }
    // A partition is as encrypted as the device it is a partition of (a RAID, a loop
    // device, a plain disk): the kernel keeps a partition's directory inside its device's.
    if dir.join("partition").exists() {
        let whole = fs::canonicalize(&dir)
            .ok()
            .and_then(|real| device_number(&real.parent()?.join("dev")));
        return match whole {
            Some(whole) if whole != (major, minor) => layer(sys, whole, depth + 1),
            _ => EncryptionState::Unknown(format!(
                "the device that partition {major}:{minor} is on cannot be read"
            )),
        };
    }
    match fs::read_to_string(dir.join("dm/uuid")) {
        Ok(uuid) if ENCRYPTING.iter().any(|format| uuid.starts_with(format)) => {
            return EncryptionState::On
        }
        // Another device-mapper target, dm-integrity and dm-verity among them: as
        // encrypted as what it sits on.
        Ok(_) => return below(sys, &dir, depth),
        Err(_) if dir.join("dm").exists() => {
            return EncryptionState::Unknown(format!(
                "the device-mapper target {major}:{minor} cannot be read"
            ))
        }
        Err(_) => {}
    }
    if dir.join("md").is_dir() {
        return below(sys, &dir, depth);
    }
    if dir.join("loop").is_dir() {
        return EncryptionState::Unknown(format!(
            "block device {major}:{minor} is a loop device backed by a file"
        ));
    }
    // A plain disk: the kernel shows no encryption under it.
    EncryptionState::Off
}

/// The combined state of the devices a device-mapper target or a RAID sits on.
fn below(sys: &Path, dir: &Path, depth: u8) -> EncryptionState {
    let slaves = dir.join("slaves");
    let entries = match fs::read_dir(&slaves) {
        Ok(entries) => entries,
        Err(e) => {
            return EncryptionState::Unknown(format!("cannot read {}: {e}", slaves.display()))
        }
    };
    let states = entries
        .map(|entry| {
            match entry
                .ok()
                .and_then(|entry| device_number(&entry.path().join("dev")))
            {
                Some(device) => layer(sys, device, depth + 1),
                None => EncryptionState::Unknown(format!(
                    "cannot read a device in {}",
                    slaves.display()
                )),
            }
        })
        .collect();
    combine(states).unwrap_or_else(|| {
        EncryptionState::Unknown(format!("{} names no device", slaves.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `/sys/dev/block`: `device` with an optional device-mapper UUID,
    /// whether it is a RAID, and the devices it sits on.
    fn device(sys: &Path, number: &str, dm_uuid: Option<&str>, md: bool, slaves: &[&str]) {
        let dir = sys.join(number);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("dev"), format!("{number}\n")).unwrap();
        if let Some(uuid) = dm_uuid {
            fs::create_dir(dir.join("dm")).unwrap();
            fs::write(dir.join("dm/uuid"), format!("{uuid}\n")).unwrap();
        }
        if md {
            fs::create_dir(dir.join("md")).unwrap();
        }
        if dm_uuid.is_some() || md {
            fs::create_dir(dir.join("slaves")).unwrap();
        }
        for (index, slave) in slaves.iter().enumerate() {
            std::os::unix::fs::symlink(sys.join(slave), dir.join(format!("slaves/s{index}")))
                .unwrap();
        }
    }

    fn at(sys: &Path, number: &str) -> EncryptionState {
        let (major, minor) = number.split_once(':').unwrap();
        layer(sys, (major.parse().unwrap(), minor.parse().unwrap()), 0)
    }

    #[test]
    fn a_dm_crypt_target_is_on_and_a_plain_partition_is_off() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path();
        device(sys, "8:2", None, false, &[]);
        device(
            sys,
            "253:0",
            Some("CRYPT-LUKS2-0123456789abcdef-luks-root"),
            false,
            &["8:2"],
        );
        assert_eq!(at(sys, "253:0"), EncryptionState::On);
        assert_eq!(at(sys, "8:2"), EncryptionState::Off);
        assert!(
            matches!(at(sys, "9:9"), EncryptionState::Unknown(why) if why.contains("9:9")),
            "a device the kernel does not list is not guessed"
        );
    }

    #[test]
    fn a_volume_is_as_encrypted_as_every_device_it_sits_on() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path();
        device(sys, "8:2", None, false, &[]);
        device(sys, "8:3", None, false, &[]);
        device(sys, "253:0", Some("CRYPT-LUKS2-a"), false, &["8:2"]);
        device(sys, "253:1", Some("CRYPT-LUKS1-b"), false, &["8:3"]);
        // LVM on LUKS, and a RAID of two LUKS devices.
        device(sys, "253:2", Some("LVM-abc"), false, &["253:0"]);
        device(sys, "9:0", None, true, &["253:0", "253:1"]);
        assert_eq!(at(sys, "253:2"), EncryptionState::On);
        assert_eq!(at(sys, "9:0"), EncryptionState::On);
        // LVM on a plain partition, and a volume that spans an encrypted and a plain one.
        device(sys, "253:3", Some("LVM-def"), false, &["8:3"]);
        device(sys, "253:4", Some("LVM-ghi"), false, &["253:0", "8:3"]);
        assert_eq!(at(sys, "253:3"), EncryptionState::Off);
        assert_eq!(at(sys, "253:4"), EncryptionState::Off);
        // A target with nothing below it, a device that is not listed, a loop device.
        device(sys, "253:5", Some("LVM-jkl"), false, &[]);
        assert!(matches!(at(sys, "253:5"), EncryptionState::Unknown(_)));
        device(sys, "253:6", Some("LVM-mno"), false, &["253:0", "7:7"]);
        assert!(matches!(at(sys, "253:6"), EncryptionState::Unknown(_)));
        fs::create_dir_all(sys.join("7:0/loop")).unwrap();
        assert!(matches!(at(sys, "7:0"), EncryptionState::Unknown(why) if why.contains("loop")));
    }

    #[test]
    fn a_loop_of_devices_ends_as_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path();
        device(sys, "253:0", Some("LVM-a"), false, &["253:1"]);
        device(sys, "253:1", Some("LVM-b"), false, &["253:0"]);
        assert!(matches!(at(sys, "253:0"), EncryptionState::Unknown(why) if why.contains("nest")));
    }

    #[test]
    fn the_mount_table_names_the_type_and_source_of_an_anonymous_device() {
        let table = "22 28 0:21 / /proc rw,nosuid shared:5 - proc proc rw\n\
                     61 28 0:48 /@home /home rw,relatime shared:30 - btrfs /dev/mapper/luks\\040home rw,ssd\n\
                     900 61 0:55 / /var/lib/x rw - overlay overlay rw,lowerdir=/a\n\
                     31 28 8:2 / / rw - ext4 /dev/sda2 rw\n";
        assert_eq!(
            mounted(table, (0, 48)),
            Some(("btrfs".into(), "/dev/mapper/luks home".into()))
        );
        assert_eq!(
            mounted(table, (0, 55)),
            Some(("overlay".into(), "overlay".into()))
        );
        assert_eq!(mounted(table, (0, 99)), None);
        assert_eq!(unescape("a\\134b\\011c\\"), "a\\b\tc\\");
    }

    #[test]
    fn a_filesystem_without_a_block_device_is_unknown_and_names_its_type() {
        let temp = tempfile::tempdir().unwrap();
        let mountinfo = temp.path().join("mountinfo");
        let meta = fs::metadata(temp.path()).unwrap();
        let (major, minor) = (libc::major(meta.dev()), libc::minor(meta.dev()));
        fs::write(
            &mountinfo,
            format!("1 0 {major}:{minor} / / rw - nfs4 server:/x rw\n"),
        )
        .unwrap();
        let probe = DmCrypt {
            sys: temp.path().join("sys"),
            btrfs: temp.path().join("btrfs"),
            mountinfo,
        };
        let state = probe.state(&temp.path().join("not/made/yet"));
        if major == 0 {
            assert!(
                matches!(&state, EncryptionState::Unknown(why) if why.contains("nfs4")),
                "{state:?}"
            );
        } else {
            // The test's folder is on a block device, which the empty stand-in does not list.
            assert!(matches!(&state, EncryptionState::Unknown(why) if why.contains("not listed")));
        }
        assert!(matches!(
            block_device(Path::new("/dev/null")),
            Err(why) if why.contains("not a block device")
        ));
    }

    /// X5 review V6: only an encrypting dm-crypt format is `On`. dm-integrity and dm-verity
    /// carry `CRYPT-` UUIDs and encrypt nothing: they are judged by what they sit on.
    #[test]
    fn only_an_encrypting_dm_crypt_format_is_on() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path();
        device(sys, "8:2", None, false, &[]);
        for (minor, uuid, on) in [
            (0, "CRYPT-LUKS1-0123-name", true),
            (1, "CRYPT-LUKS2-0123-name", true),
            (2, "CRYPT-PLAIN-name", true),
            (3, "CRYPT-TCRYPT-name", true),
            (4, "CRYPT-BITLK-0123-name", true),
            (5, "CRYPT-FVAULT2-0123-name", true),
            (6, "CRYPT-INTEGRITY-0123-name", false),
            (7, "CRYPT-VERITY-0123-name", false),
            (8, "CRYPT-", false),
            (9, "CRYPT-SOMETHING-NEW-name", false),
            (10, "LVM-CRYPT-LUKS2-lookalike", false),
        ] {
            let number = format!("253:{minor}");
            device(sys, &number, Some(uuid), false, &["8:2"]);
            let expected = if on {
                EncryptionState::On
            } else {
                // Not an encrypting target: the plain partition below it decides.
                EncryptionState::Off
            };
            assert_eq!(at(sys, &number), expected, "{uuid}");
        }
        // dm-integrity below LUKS2 (authenticated encryption) is on by the LUKS2 target,
        // and dm-verity over a LUKS device is as encrypted as that device.
        device(sys, "253:20", Some("CRYPT-INTEGRITY-x"), false, &["8:2"]);
        device(sys, "253:21", Some("CRYPT-LUKS2-y"), false, &["253:20"]);
        device(sys, "253:22", Some("CRYPT-VERITY-z"), false, &["253:21"]);
        assert_eq!(at(sys, "253:21"), EncryptionState::On);
        assert_eq!(at(sys, "253:22"), EncryptionState::On);
        // A device-mapper directory whose UUID cannot be read is not guessed.
        fs::create_dir_all(sys.join("253:30/dm")).unwrap();
        assert!(matches!(at(sys, "253:30"), EncryptionState::Unknown(_)));
    }

    /// A partition is judged by the device it is a partition of: a partition of a RAID of
    /// LUKS devices is on, a partition of a loop device is not known, a partition of a
    /// plain disk is off, and a partition whose device cannot be read is not known.
    #[test]
    fn a_partition_is_as_encrypted_as_its_device() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("devices");
        let sys = temp.path().join("block");
        fs::create_dir_all(&sys).unwrap();
        // The kernel's layout: a device's directory holds its partitions' directories, and
        // `/sys/dev/block/<number>` links to each.
        let whole = |name: &str, number: &str| {
            let dir = real.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("dev"), format!("{number}\n")).unwrap();
            std::os::unix::fs::symlink(&dir, sys.join(number)).unwrap();
            dir
        };
        let partition = |of: &Path, name: &str, number: &str| {
            let dir = of.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("dev"), format!("{number}\n")).unwrap();
            fs::write(dir.join("partition"), "1\n").unwrap();
            std::os::unix::fs::symlink(&dir, sys.join(number)).unwrap();
        };
        let sda = whole("sda", "8:0");
        partition(&sda, "sda1", "8:1");
        partition(&sda, "sda2", "8:2");
        assert_eq!(at(&sys, "8:1"), EncryptionState::Off);
        for (name, number) in [("dm-0", "253:0"), ("dm-1", "253:1")] {
            let dm = whole(name, number);
            fs::create_dir_all(dm.join("dm")).unwrap();
            fs::write(dm.join("dm/uuid"), "CRYPT-LUKS2-x\n").unwrap();
        }
        let md = whole("md0", "9:0");
        fs::create_dir_all(md.join("md")).unwrap();
        fs::create_dir_all(md.join("slaves")).unwrap();
        for name in ["dm-0", "dm-1"] {
            std::os::unix::fs::symlink(real.join(name), md.join("slaves").join(name)).unwrap();
        }
        partition(&md, "md0p1", "259:1");
        assert_eq!(at(&sys, "9:0"), EncryptionState::On);
        assert_eq!(at(&sys, "259:1"), EncryptionState::On);
        // One member of the RAID is a plain partition: the partition on it is off.
        std::os::unix::fs::symlink(sda.join("sda2"), md.join("slaves/sda2")).unwrap();
        assert_eq!(at(&sys, "259:1"), EncryptionState::Off);
        let lo = whole("loop0", "7:0");
        fs::create_dir_all(lo.join("loop")).unwrap();
        partition(&lo, "loop0p1", "259:2");
        assert!(matches!(at(&sys, "259:2"), EncryptionState::Unknown(why) if why.contains("loop")));
        // A partition whose device has no readable number.
        let odd = real.join("odd");
        fs::create_dir_all(odd.join("odd1")).unwrap();
        fs::write(odd.join("odd1/dev"), "259:3\n").unwrap();
        fs::write(odd.join("odd1/partition"), "1\n").unwrap();
        std::os::unix::fs::symlink(odd.join("odd1"), sys.join("259:3")).unwrap();
        assert!(matches!(at(&sys, "259:3"), EncryptionState::Unknown(_)));
    }

    /// A Btrfs volume is on only if every device the kernel lists for it is.
    #[test]
    fn a_btrfs_volume_is_judged_by_every_one_of_its_devices() {
        let temp = tempfile::tempdir().unwrap();
        let sys = temp.path().join("block");
        let btrfs = temp.path().join("btrfs");
        device(&sys, "8:2", None, false, &[]);
        device(&sys, "8:3", None, false, &[]);
        device(&sys, "253:0", Some("CRYPT-LUKS2-a"), false, &["8:2"]);
        device(&sys, "253:1", Some("CRYPT-LUKS2-b"), false, &["8:3"]);
        let volume = |fsid: &str, members: &[&str]| {
            let devices = btrfs.join(fsid).join("devices");
            fs::create_dir_all(&devices).unwrap();
            for (index, member) in members.iter().enumerate() {
                std::os::unix::fs::symlink(sys.join(member), devices.join(format!("d{index}")))
                    .unwrap();
            }
        };
        volume("11111111-aaaa", &["253:0", "253:1"]);
        volume("22222222-bbbb", &["253:0", "8:3"]);
        volume("33333333-cccc", &["8:2"]);
        // Found by any of its members; two LUKS devices: on.
        assert_eq!(btrfs_volume(&sys, &btrfs, (253, 1)), EncryptionState::On);
        // The second volume shares 253:0 with the first in this stand-in; asked by its
        // plain member it is off, because one of its devices certainly is not encrypted.
        assert_eq!(btrfs_volume(&sys, &btrfs, (8, 3)), EncryptionState::Off);
        assert_eq!(btrfs_volume(&sys, &btrfs, (8, 2)), EncryptionState::Off);
        // A volume the kernel does not list, a member that cannot be read, no listing.
        assert!(matches!(
            btrfs_volume(&sys, &btrfs, (253, 9)),
            EncryptionState::Unknown(why) if why.contains("not listed")
        ));
        fs::create_dir_all(btrfs.join("44444444-dddd/devices/gone")).unwrap();
        device(&sys, "253:2", Some("CRYPT-LUKS2-c"), false, &["8:2"]);
        std::os::unix::fs::symlink(
            sys.join("253:2"),
            btrfs.join("44444444-dddd/devices/present"),
        )
        .unwrap();
        assert!(matches!(
            btrfs_volume(&sys, &btrfs, (253, 2)),
            EncryptionState::Unknown(why) if why.contains("cannot be read")
        ));
        assert!(matches!(
            btrfs_volume(&sys, &temp.path().join("no-btrfs"), (253, 0)),
            EncryptionState::Unknown(_)
        ));
    }

    /// This machine's own answer: never a panic, and a reason whenever it is not known.
    #[test]
    fn the_real_probe_answers_for_a_folder_of_this_machine() {
        let probe = disk_encryption().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let state = probe.state(temp.path());
        println!("disk encryption of {}: {state:?}", temp.path().display());
        if let EncryptionState::Unknown(why) = state {
            assert!(!why.is_empty());
        }
    }
}
