use std::os::unix::fs::PermissionsExt;

use super::*;

fn folder() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    (dir, path)
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_file_is_published_owner_only_with_no_temporary_left() {
    let (_dir, root) = folder();
    publish_one(&root, "office.json", b"one\n", false).unwrap();
    assert_eq!(
        read_one(&root, "office.json", 64).unwrap().unwrap(),
        b"one\n"
    );
    let mode = fs::metadata(root.join("office.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(names(&root), ["office.json"]);
    // Replacing.
    publish_one(&root, "office.json", b"two\n", false).unwrap();
    assert_eq!(
        read_one(&root, "office.json", 64).unwrap().unwrap(),
        b"two\n"
    );
    assert_eq!(read_one(&root, "missing.json", 64).unwrap(), None);
}

#[test]
fn an_exclusive_file_is_never_replaced_and_the_same_bytes_are_success() {
    let (_dir, root) = folder();
    publish_one(&root, "intent.json", b"first", true).unwrap();
    publish_one(&root, "intent.json", b"first", true).unwrap();
    let refused = publish_one(&root, "intent.json", b"second", true);
    assert!(refused.unwrap_err().to_string().contains("other bytes"));
    assert_eq!(
        read_one(&root, "intent.json", 64).unwrap().unwrap(),
        b"first"
    );
    assert_eq!(names(&root), ["intent.json"]);
}

#[test]
fn a_temporary_name_someone_else_holds_is_never_written_through() {
    let (_dir, root) = folder();
    let outside = root.join("outside");
    fs::write(&outside, b"kept").unwrap();
    // Every name this process could pick next is a link: the publication skips them.
    let pid = std::process::id();
    let next = ONE_TEMP.load(Ordering::Relaxed);
    for n in next..next + 8 {
        let name = format!("{TEMP_PREFIX}one-{pid}-{n}");
        std::os::unix::fs::symlink(&outside, root.join(name)).unwrap();
    }
    publish_one(&root, "office.json", b"mine", false).unwrap();
    assert_eq!(fs::read(&outside).unwrap(), b"kept");
    assert_eq!(
        read_one(&root, "office.json", 64).unwrap().unwrap(),
        b"mine"
    );
}

#[test]
fn links_large_files_and_bad_names_are_refused() {
    let (_dir, root) = folder();
    let outside = root.join("outside");
    fs::write(&outside, b"x").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.json")).unwrap();
    assert!(read_one(&root, "link.json", 64).is_err());
    fs::write(root.join("big.json"), vec![b' '; 65]).unwrap();
    assert!(read_one(&root, "big.json", 64).is_err());
    assert!(read_one(&root, "big.json", 65).is_ok());
    for name in ["", "..", "a/b", "Office.json", ".store-tmp-x"] {
        assert!(publish_one(&root, name, b"x", false).is_err(), "{name:?}");
    }
    assert!(publish_one(Path::new("relative"), "a.json", b"x", false).is_err());
    let file = root.join("plain");
    fs::write(&file, b"x").unwrap();
    assert!(publish_one(&file, "a.json", b"x", false).is_err());
}
