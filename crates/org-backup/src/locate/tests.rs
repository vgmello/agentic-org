use super::*;

#[test]
fn only_the_regular_file_still_at_the_path_counts_as_locked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("office.lock");
    let held = take_lock(&path, "office".into()).expect("a fresh lock");
    assert!(is_the_file_at(&held, &path));

    // Moved away, with a link to it put in its place: the name no longer is the file.
    let moved = dir.path().join("moved.lock");
    std::fs::rename(&path, &moved).unwrap();
    assert!(!is_the_file_at(&held, &path));
    std::os::unix::fs::symlink(&moved, &path).unwrap();
    assert!(!is_the_file_at(&held, &path));

    // Another file at the name, and no file at all.
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"").unwrap();
    assert!(!is_the_file_at(&held, &path));
    std::fs::remove_file(&path).unwrap();
    assert!(!is_the_file_at(&held, &path));

    // A lock is never taken through a link.
    let link = dir.path().join("link.lock");
    std::os::unix::fs::symlink(&moved, &link).unwrap();
    assert!(matches!(
        take_lock(&link, "office".into()),
        Err(BackupError::Io { .. })
    ));
    // And the moved file is still locked by the first handle.
    assert!(matches!(
        take_lock(&moved, "office".into()),
        Err(BackupError::Live { .. })
    ));
}
