use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn updates_require_the_current_retained_control_handle() {
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
    let first = directory.publish("control", b"first", None).unwrap();
    let second = directory
        .publish("control", b"second", Some(&first))
        .unwrap();
    assert!(directory
        .publish("control", b"stale", Some(&first))
        .is_err());
    assert!(directory.publish("control", b"unclaimed", None).is_err());
    assert_eq!(
        std::fs::read(directory.path().join("control")).unwrap(),
        b"second"
    );
    directory
        .publish("control", b"third", Some(&second))
        .unwrap();
    assert_eq!(
        std::fs::read(directory.path().join("control")).unwrap(),
        b"third"
    );
}

#[test]
fn updates_refuse_control_entries_replaced_since_the_checked_read() {
    use std::os::unix::fs::symlink;
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
    for replacement in ["symlink", "hardlink", "different-inode"] {
        for during_staging in [false, true] {
            let directory = root
                .child(&format!("{replacement}-{during_staging}"), true)
                .unwrap();
            directory.publish("control", b"original", None).unwrap();
            let before = directory.read("control", 64).unwrap().unwrap();
            assert_eq!(&*before, b"original");
            let control = directory.path().join("control");
            let target = directory.path().join("target");
            let identity = std::cell::Cell::new(None);
            let replace = || {
                std::fs::write(&target, b"replacement").unwrap();
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
                std::fs::rename(&control, directory.path().join("previous")).unwrap();
                match replacement {
                    "symlink" => symlink(&target, &control).unwrap(),
                    "hardlink" => std::fs::hard_link(&target, &control).unwrap(),
                    _ => std::fs::rename(&target, &control).unwrap(),
                }
                identity.set(Some(std::fs::symlink_metadata(&control).unwrap().ino()));
            };
            let result = if during_staging {
                directory.publish_after_staging("control", b"update", Some(&before), replace)
            } else {
                replace();
                directory.publish("control", b"update", Some(&before))
            };
            assert!(
                result.is_err(),
                "{replacement} was overwritten, during_staging={during_staging}"
            );
            assert_eq!(
                Some(std::fs::symlink_metadata(&control).unwrap().ino()),
                identity.get()
            );
            assert_eq!(std::fs::read(&control).unwrap(), b"replacement");
        }
    }
}

#[test]
fn checked_reads_refuse_control_entries_replaced_after_open() {
    use std::os::unix::fs::symlink;
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
    for replacement in ["symlink", "hardlink", "different-inode"] {
        let directory = root.child(replacement, true).unwrap();
        directory.publish("control", b"original", None).unwrap();
        let held = directory.open_file("control", libc::O_RDONLY).unwrap();
        let path = directory.path().to_owned();
        let (ready, replaced) = std::sync::mpsc::channel();
        let mutator = std::thread::spawn(move || {
            let control = path.join("control");
            let target = path.join("target");
            std::fs::write(&target, b"replacement").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::rename(&control, path.join("previous")).unwrap();
            match replacement {
                "symlink" => symlink(&target, &control).unwrap(),
                "hardlink" => std::fs::hard_link(&target, &control).unwrap(),
                _ => std::fs::rename(&target, &control).unwrap(),
            }
            ready.send(()).unwrap();
        });
        replaced.recv().unwrap();
        assert!(
            matches!(
                directory.read_from("control", held, 64),
                Err(Error::Unsafe(_))
            ),
            "{replacement} was accepted after opening the original inode"
        );
        mutator.join().unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("control")).unwrap(),
            b"replacement"
        );
    }
}

/// X5 review, fourth round, P2-6: a root opened for a question before its reservation
/// is reached without following a link, and only a folder the probe made is removed
/// again, and only while its name still holds that folder.
#[test]
fn a_root_probe_removes_only_the_folder_it_made() {
    let fixture = super::super::test_directory();
    let base = fixture.path().canonicalize().unwrap();
    let made = base.join("new");
    let probe = RootProbe::open(&made).unwrap();
    assert!(made.is_dir());
    drop(probe);
    assert!(!made.exists(), "the folder the probe made was left");
    // An existing folder stays, and so does what is in it.
    std::fs::create_dir(&made).unwrap();
    std::fs::write(made.join("x"), b"x").unwrap();
    drop(RootProbe::open(&made).unwrap());
    assert!(made.join("x").exists());
    // A folder put in the place of the one the probe made is not removed.
    let replaced = base.join("replaced");
    let probe = RootProbe::open(&replaced).unwrap();
    std::fs::rename(&replaced, base.join("moved")).unwrap();
    std::fs::create_dir(&replaced).unwrap();
    drop(probe);
    assert!(replaced.is_dir() && base.join("moved").is_dir());
    // A link on the way is never followed, and nothing is made through it.
    std::os::unix::fs::symlink(&base, base.join("link")).unwrap();
    assert!(RootProbe::open(&base.join("link/through")).is_err());
    assert!(!base.join("through").exists());
}

/// What each directory is checked for is the platform's rule (`org_platform::fs`, tested
/// there); the walk's part is to check every directory from `/` in order, and only the
/// last one as the root.
#[test]
fn the_walk_checks_every_directory_in_order_and_only_the_last_as_the_root() {
    let fixture = super::super::test_directory();
    let root = fixture.path().canonicalize().unwrap();
    let mut expected: Vec<_> = root.ancestors().map(Path::to_owned).collect();
    expected.reverse();
    for is_root in [false, true] {
        let mut checked = Vec::new();
        Directory::walk_with_probe(&root, false, is_root, |file, path, root| {
            assert_eq!(
                Identity::of(file).unwrap(),
                Identity::of(&File::open(path).unwrap()).unwrap()
            );
            checked.push((path.to_owned(), root));
            Ok(())
        })
        .unwrap();
        let paths: Vec<_> = checked.iter().map(|(path, _)| path.clone()).collect();
        assert_eq!(paths, expected);
        for (path, as_root) in &checked {
            assert_eq!(*as_root, is_root && path == &root, "{}", path.display());
        }
    }
    // A refusal of any directory on the way ends the walk with that refusal.
    for component in &expected {
        let walked = Directory::walk_with_probe(&root, false, true, |_, path, _| {
            if path == component {
                Err(org_platform::fs::FilesystemRefusal::CaseInsensitive(path.to_owned()).into())
            } else {
                Ok(())
            }
        });
        assert!(matches!(walked,
            Err(Error::CaseInsensitive(path)) if &path == component));
    }
}

#[test]
fn unsupported_new_root_is_refused_before_creation() {
    let fixture = super::super::test_directory();
    let root = fixture.path().canonicalize().unwrap().join("new");
    let result = Directory::walk_with_probe(&root, true, true, |_, path, is_root| {
        if is_root {
            Err(Error::UnsupportedFilesystem {
                path: path.to_owned(),
                kind: 0x0102_1994,
            })
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(Error::UnsupportedFilesystem { .. })));
    assert!(!root.exists());
}

/// The native fixture mounts a second disk below a supported instance root.
#[test]
fn native_instance_children_refuse_a_different_device() {
    let Some(path) = std::env::var_os("ORG_INSTANCE_NESTED_MOUNT_FIXTURE") else {
        return;
    };
    let root = Directory::open_root(Path::new(&path), false).unwrap();
    let child_path = root.path().join("nested");
    assert_ne!(
        std::fs::metadata(&child_path).unwrap().dev(),
        root.identity.device
    );
    assert!(matches!(
        root.child("nested", false),
        Err(Error::Unsafe(
            "instance descendants must remain on the root device"
        ))
    ));
    assert!(!child_path.join("untouched").exists());
}

/// The platform's refusals (`org_platform::fs`, where the rules are tested) keep the
/// data and the wording this module reports.
#[test]
fn platform_refusals_keep_their_instance_diagnostics() {
    use org_platform::fs::FilesystemRefusal as Refusal;
    let path = Path::new("/disk/folded-parent");
    let error = Error::from(Refusal::CaseInsensitive(path.to_owned()));
    assert!(matches!(&error, Error::CaseInsensitive(component) if component == path));
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("/disk/folded-parent"));
    assert!(diagnostic.contains("case-insensitive directories aren't supported on Linux"));

    let path = Path::new("/disk/instance");
    for (probe, errno) in [
        ("FS_IOC_GETFLAGS", libc::EACCES),
        ("XFS_IOC_FSGEOMETRY", libc::EIO),
        ("fstatfs", libc::EACCES),
    ] {
        let error = Error::from(Refusal::Probe {
            path: path.to_owned(),
            probe,
            source: io::Error::from_raw_os_error(errno),
        });
        assert!(
            matches!(&error, Error::FilesystemProbe { path: reported, probe: named, source } if reported == path && *named == probe && source.raw_os_error() == Some(errno))
        );
        assert!(error.to_string().contains("/disk/instance"));
        assert!(error.to_string().contains(probe));
    }
    for kind in [0, 0x0102_1994, u64::MAX] {
        let error = Error::from(Refusal::UnsupportedFilesystem {
            path: path.to_owned(),
            kind,
        });
        assert!(
            matches!(&error, Error::UnsupportedFilesystem { path: reported, kind: reported_kind } if reported == path && *reported_kind == kind)
        );
        assert!(error.to_string().contains(&format!("type {kind:#x}")));
    }
    assert!(matches!(
        Error::from(Refusal::MemoryBacked {
            path: path.to_owned(),
            kind: 0x0102_1994
        }),
        Error::Unsafe("user temp base must be disk-backed")
    ));
}

#[test]
fn concurrent_lock_file_creation_never_loses_the_existing_entry() {
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
    let directories: Vec<_> = (0..128)
        .map(|index| root.child(&index.to_string(), true).unwrap())
        .collect();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let run = || {
            let mut results = Vec::new();
            for directory in &directories {
                barrier.wait();
                results.push(directory.lock_file("registry.lock").map(drop));
                barrier.wait();
            }
            results
        };
        let first = scope.spawn(run);
        let second = scope.spawn(run);
        for result in first
            .join()
            .unwrap()
            .into_iter()
            .chain(second.join().unwrap())
        {
            assert!(matches!(result, Ok(()) | Err(Error::Busy(_))), "{result:?}");
        }
    });
}

#[test]
fn dropping_lock_owners_unlocks_even_while_a_duplicate_fd_survives() {
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = temporary.path().canonicalize().unwrap();
    let mut root = Directory::open(&path, false).unwrap();
    let held = root.lock_file("registry.lock").unwrap();
    let inherited_file = held.try_clone().unwrap();
    drop(held);
    root.lock_file("registry.lock").unwrap();
    drop(inherited_file);
    root.lock_root().unwrap();
    let inherited_root = root.file.try_clone().unwrap();
    drop(root);
    Directory::open(&path, false).unwrap().lock_root().unwrap();
    drop(inherited_root);
}

#[test]
fn relative_file_capabilities_cannot_escape_the_opened_directory() {
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let parent = Directory::open(&temporary.path().canonicalize().unwrap(), false).unwrap();
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "/escape",
        "nested/file",
        "trailing/",
        "a\0b",
    ] {
        assert!(parent.claim_child(name).is_err(), "{name:?}");
        assert!(parent.read(name, 10).is_err(), "{name:?}");
    }
    assert!(parent.is_empty().unwrap());
}

#[test]
fn verifying_a_held_lock_refuses_a_symlink_back_to_the_same_inode() {
    use std::os::unix::fs::symlink;
    let temporary = super::super::test_directory();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = temporary.path().canonicalize().unwrap();
    let directory = Directory::open(&path, false).unwrap();
    let held = directory.lock_file("registry.lock").unwrap();
    std::fs::rename(path.join("registry.lock"), path.join("original")).unwrap();
    symlink(path.join("original"), path.join("registry.lock")).unwrap();
    assert!(directory.verify_file("registry.lock", &held).is_err());
}
