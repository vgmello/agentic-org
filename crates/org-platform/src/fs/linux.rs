//! The GNU/Linux filesystem adapter.

use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use super::FilesystemRefusal;

pub(super) fn full_sync(file: &File) -> io::Result<()> {
    file.sync_all()?;
    #[cfg(any(test, feature = "test-support"))]
    super::testing::COUNTS.with(|counts| {
        counts.update(|mut now| {
            now.fsync += 1;
            now
        })
    });
    Ok(())
}

pub(super) fn rename_noreplace_at(
    from_dir: &File,
    from: &CStr,
    to_dir: &File,
    to: &CStr,
) -> io::Result<()> {
    rename_noreplace_raw(from_dir.as_raw_fd(), from, to_dir.as_raw_fd(), to)
}

pub(super) fn rename_noreplace(from: &CStr, to: &CStr) -> io::Result<()> {
    rename_noreplace_raw(libc::AT_FDCWD, from, libc::AT_FDCWD, to)
}

fn rename_noreplace_raw(
    from_dir: libc::c_int,
    from: &CStr,
    to_dir: libc::c_int,
    to: &CStr,
) -> io::Result<()> {
    // SAFETY: each descriptor is a live directory the caller retains, or `AT_FDCWD`,
    // and both names are NUL-terminated strings that live across the call. The
    // exclusive rename never overwrites an existing entry.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from_dir,
            from.as_ptr(),
            to_dir,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn set_errno(value: i32) -> io::Result<()> {
    // SAFETY: libc returns this thread's errno pointer; no shared memory access.
    unsafe {
        *libc::__errno_location() = value;
    }
    Ok(())
}

pub(super) fn device_and_inode(stat: &libc::stat) -> io::Result<(u64, u64)> {
    Ok((stat.st_dev, stat.st_ino))
}

pub(super) fn check_instance_root(
    dir: &File,
    path: &Path,
    root: bool,
) -> Result<(), FilesystemRefusal> {
    check_component(dir, path, root, &NativeProbe)
}

pub(super) fn require_disk(dir: &File, path: &Path) -> Result<(), FilesystemRefusal> {
    // Linux UAPI include/uapi/linux/magic.h defines ramfs/tmpfs/hugetlbfs.
    // Fail closed on known memory filesystems; setup supplies the explicit disk base.
    let kind = NativeProbe
        .filesystem(dir, path)
        .map_err(|source| FilesystemRefusal::Probe {
            path: path.to_owned(),
            probe: "fstatfs",
            source,
        })?;
    if matches!(kind, 0x01021994 | 0x858458f6 | 0x958458f6) {
        return Err(FilesystemRefusal::MemoryBacked {
            path: path.to_owned(),
            kind,
        });
    }
    Ok(())
}

/// What the instance-root rules ask of the kernel, so tests can answer for it.
trait Probe {
    fn flags(&self, file: &File, path: &Path) -> io::Result<libc::c_int>;
    fn filesystem(&self, file: &File, path: &Path) -> io::Result<u64>;
    fn xfs(&self, file: &File, path: &Path) -> io::Result<u32>;
}

struct NativeProbe;

impl Probe for NativeProbe {
    fn flags(&self, file: &File, _: &Path) -> io::Result<libc::c_int> {
        let mut flags = 0;
        // SAFETY: FS_IOC_GETFLAGS writes into our valid output for a retained directory.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::FS_IOC_GETFLAGS, &mut flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(flags)
    }
    fn filesystem(&self, file: &File, _: &Path) -> io::Result<u64> {
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: fstatfs initializes stats on success for this retained descriptor.
        if unsafe { libc::fstatfs(file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded.
        Ok(unsafe { stats.assume_init() }.f_type as u32 as u64)
    }
    fn xfs(&self, file: &File, _: &Path) -> io::Result<u32> {
        xfs_geometry_flags(file)
    }
}

fn check_component(
    file: &File,
    path: &Path,
    root: bool,
    probe: &impl Probe,
) -> Result<(), FilesystemRefusal> {
    // Ancestor mount types may be tmpfs/autofs/etc.; their casefold flags still matter.
    check_flags(path, probe.flags(file, path))?;
    if root {
        let kind = probe
            .filesystem(file, path)
            .map_err(|source| FilesystemRefusal::Probe {
                path: path.to_owned(),
                probe: "fstatfs",
                source,
            })?;
        require_root_filesystem(path, kind)?;
        if kind == 0x5846_5342 {
            check_xfs_geometry(path, probe.xfs(file, path))?;
        }
    }
    Ok(())
}

fn require_root_filesystem(path: &Path, kind: u64) -> Result<(), FilesystemRefusal> {
    if !matches!(
        kind,
        0xEF53 | 0x5846_5342 | 0x9123_683E | 0xF2F5_2010 | 0x794C_7630
    ) {
        return Err(FilesystemRefusal::UnsupportedFilesystem {
            path: path.to_owned(),
            kind,
        });
    }
    Ok(())
}

fn check_flags(path: &Path, flags: io::Result<libc::c_int>) -> Result<(), FilesystemRefusal> {
    let flags = match flags {
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTTY | libc::EOPNOTSUPP)) => 0,
        result => result.map_err(|source| FilesystemRefusal::Probe {
            path: path.to_owned(),
            probe: "FS_IOC_GETFLAGS",
            source,
        })?,
    };
    reject_casefold(path, flags)
}

fn reject_casefold(path: &Path, flags: libc::c_int) -> Result<(), FilesystemRefusal> {
    // Linux UAPI include/uapi/linux/fs.h: FS_CASEFOLD_FL.
    if flags & 0x4000_0000 != 0 {
        return Err(FilesystemRefusal::CaseInsensitive(path.to_owned()));
    }
    Ok(())
}

// Linux fs/xfs/libxfs/xfs_fs.h, xfs_fsop_geom. The v5 UAPI is 256 bytes;
// newer geometry fields occupy the reserved tail, which this adapter never reads.
#[repr(C)]
struct XfsGeometry {
    dimensions: [u32; 8],
    blocks: [u64; 4],
    uuid: [u8; 16],
    stripe: [u32; 2],
    version: i32,
    flags: u32,
    sectors: [u32; 4],
    health: [u32; 2],
    reserved: [u64; 17],
}

const _: () = {
    assert!(std::mem::size_of::<XfsGeometry>() == 256);
    assert!(std::mem::offset_of!(XfsGeometry, flags) == 92);
};

fn xfs_geometry_flags(file: &File) -> io::Result<u32> {
    let mut geometry = std::mem::MaybeUninit::<XfsGeometry>::zeroed();
    let request = libc::_IOR::<XfsGeometry>(u32::from(b'X'), 126);
    // SAFETY: request encodes this UAPI buffer's size; the descriptor and buffer are live.
    if unsafe { libc::ioctl(file.as_raw_fd(), request, geometry.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful FSGEOMETRY initialized the zeroed UAPI output.
    Ok(unsafe { geometry.assume_init() }.flags)
}

fn check_xfs_geometry(path: &Path, flags: io::Result<u32>) -> Result<(), FilesystemRefusal> {
    let flags = flags.map_err(|source| FilesystemRefusal::Probe {
        path: path.to_owned(),
        probe: "XFS_IOC_FSGEOMETRY",
        source,
    })?;
    // XFS_FSOP_GEOM_FLAGS_DIRV2CI: legacy ASCII case-insensitive lookups.
    if flags & (1 << 12) != 0 {
        return Err(FilesystemRefusal::CaseInsensitive(path.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The walk is the consumer's (`org-host`'s instance directory): every directory
    /// from `/` to the root in order, and only the last one as the root.
    #[test]
    fn walked_ancestors_probe_flags_but_only_the_root_probes_filesystem_and_xfs() {
        use std::cell::RefCell;
        struct Fake<'a> {
            root: &'a Path,
            folded: Option<&'a Path>,
            kind: u64,
            xfs_flags: u32,
            flags: RefCell<Vec<PathBuf>>,
            xfs_calls: RefCell<Vec<PathBuf>>,
        }
        impl Probe for Fake<'_> {
            fn flags(&self, _: &File, path: &Path) -> io::Result<i32> {
                self.flags.borrow_mut().push(path.to_owned());
                if self.folded == Some(path) {
                    Ok(0x4000_0000)
                } else if path == self.root {
                    Ok(0)
                } else {
                    Err(io::Error::from_raw_os_error(libc::ENOTTY))
                }
            }
            fn filesystem(&self, _: &File, path: &Path) -> io::Result<u64> {
                // Any call on an ancestor (which may be unsupported tmpfs/autofs)
                // is a policy/walk wiring regression, not a classifier failure.
                assert_eq!(path, self.root);
                Ok(self.kind)
            }
            fn xfs(&self, _: &File, path: &Path) -> io::Result<u32> {
                self.xfs_calls.borrow_mut().push(path.to_owned());
                Ok(self.xfs_flags)
            }
        }
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let probe = |folded, kind, xfs_flags| Fake {
            root: &root,
            folded,
            kind,
            xfs_flags,
            flags: RefCell::default(),
            xfs_calls: RefCell::default(),
        };
        let mut expected: Vec<_> = root.ancestors().map(Path::to_owned).collect();
        expected.reverse();
        let walk = |probe: &Fake<'_>| {
            for component in &expected {
                let directory = File::open(component).unwrap();
                check_component(&directory, component, component == &root, probe)?;
            }
            Ok::<_, FilesystemRefusal>(())
        };
        let ext4 = probe(None, 0xEF53, 0);
        walk(&ext4).unwrap();
        assert_eq!(*ext4.flags.borrow(), expected);
        for component in &expected {
            assert!(matches!(walk(&probe(Some(component), 0xEF53, 0)),
                Err(FilesystemRefusal::CaseInsensitive(path)) if &path == component));
        }
        assert!(matches!(walk(&probe(None, 0x0102_1994, 0)),
            Err(FilesystemRefusal::UnsupportedFilesystem { path, .. }) if path == root));
        let xfs = probe(None, 0x5846_5342, 0);
        walk(&xfs).unwrap();
        assert_eq!(*xfs.xfs_calls.borrow(), vec![root.clone()]);
        assert!(matches!(walk(&probe(None, 0x5846_5342, 1 << 12)),
            Err(FilesystemRefusal::CaseInsensitive(path)) if path == root));
    }

    #[test]
    fn disk_validation_refuses_a_real_memory_filesystem() {
        let temporary = tempfile::tempdir_in("/dev/shm").unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let directory = File::open(&path).unwrap();
        assert!(matches!(
            require_disk(&directory, &path),
            Err(FilesystemRefusal::MemoryBacked { path: reported, kind: 0x0102_1994 })
                if reported == path
        ));
    }

    #[test]
    fn casefold_refusal_names_the_unsupported_component() {
        let path = Path::new("/disk/folded-parent");
        for flags in [0x4000_0000, 0x4008_0000] {
            let error = check_flags(path, Ok(flags)).unwrap_err();
            assert!(
                matches!(&error, FilesystemRefusal::CaseInsensitive(component) if component == path)
            );
            assert!(error.to_string().contains("/disk/folded-parent"));
        }
        check_flags(path, Ok(0)).unwrap();
        check_flags(path, Ok(0x0008_0000)).unwrap();
    }

    #[test]
    fn linux_filesystem_support_is_explicit_and_ioctl_errors_fail_closed() {
        let path = Path::new("/disk/instance");
        for kind in [0xEF53, 0x5846_5342, 0x9123_683E, 0xF2F5_2010, 0x794C_7630] {
            require_root_filesystem(path, kind).unwrap();
            check_flags(path, Ok(0)).unwrap();
            for errno in [libc::ENOTTY, libc::EOPNOTSUPP] {
                check_flags(path, Err(io::Error::from_raw_os_error(errno))).unwrap();
            }
            let error =
                check_flags(path, Err(io::Error::from_raw_os_error(libc::EACCES))).unwrap_err();
            assert!(
                matches!(&error, FilesystemRefusal::Probe { path: reported, probe: "FS_IOC_GETFLAGS", source } if reported == path && source.raw_os_error() == Some(libc::EACCES))
            );
            assert!(error.to_string().contains("/disk/instance"));
            assert!(error.to_string().contains("FS_IOC_GETFLAGS"));
        }
        for kind in [
            0,
            0x4d44,
            0x2011_BAB0,
            0xff53_4d42,
            0x6969,
            0x6573_5546,
            0x5346_544e,
            0x0102_1994,
            0x2FC1_2FC1,
            u64::MAX,
        ] {
            let error = require_root_filesystem(path, kind).unwrap_err();
            assert!(
                matches!(&error, FilesystemRefusal::UnsupportedFilesystem { path: reported, kind: reported_kind } if reported == path && *reported_kind == kind)
            );
            assert!(error.to_string().contains(&format!("type {kind:#x}")));
        }
    }

    #[test]
    fn xfs_requires_successful_geometry_without_ascii_casefolding() {
        let path = Path::new("/disk/xfs");
        for flags in [1 << 12, (1 << 12) | (1 << 15)] {
            assert!(matches!(
                check_xfs_geometry(path, Ok(flags)),
                Err(FilesystemRefusal::CaseInsensitive(component)) if component == path
            ));
        }
        check_xfs_geometry(path, Ok(0)).unwrap();
        check_xfs_geometry(path, Ok(1 << 15)).unwrap();
        for errno in [libc::ENOTTY, libc::EOPNOTSUPP, libc::EACCES, libc::EIO] {
            let error =
                check_xfs_geometry(path, Err(io::Error::from_raw_os_error(errno))).unwrap_err();
            assert!(
                matches!(&error, FilesystemRefusal::Probe { path: reported, probe: "XFS_IOC_FSGEOMETRY", source } if reported == path && source.raw_os_error() == Some(errno))
            );
            assert!(error.to_string().contains("/disk/xfs"));
        }
    }
}
