use super::*;
use std::time::Instant;

/// This machine's own table with a Docker daemon and a container running, exactly
/// as the kernel wrote it: Docker's network namespace is a mount whose root is
/// `net:[4026532268]`, and its container's root is an overlay.
const DOCKER_HOST: &str = include_str!("docker_host.mountinfo");

/// The table a container sees with two named volumes at `/data` and `/secrets`
/// (`docker run -v x3probe-data:/data -v x3probe-secrets:/secrets`), as written.
const CONTAINER_TWO_VOLUMES: &str = include_str!("container_two_volumes.mountinfo");

/// The table a container sees with `-v D/agentic:/data -v D/agentic/secrets:/secrets`:
/// the secrets directory is a directory of the data volume.
const CONTAINER_SECRETS_INSIDE_DATA: &str = include_str!("container_secrets_inside_data.mountinfo");

/// The table a container sees with two named volumes at `/data` and `/secrets` and a
/// folder of the data volume mounted at `/secrets/dev-keys`
/// (`--mount type=volume,src=D,dst=/secrets/dev-keys,volume-subpath=devkeys`), as
/// written. All three are on one filesystem.
const CONTAINER_MOUNT_INSIDE_SECRETS: &str =
    include_str!("container_mount_inside_secrets.mountinfo");

/// The table a container sees with two named volumes at `/data` and `/secrets`, the
/// secrets volume mounted a second time inside the data volume (`-v S:/data/extra`)
/// and a folder of it there as well (`--mount
/// type=volume,src=S,dst=/data/instance-keys,volume-subpath=office-keys`), as written.
const CONTAINER_SECRETS_MOUNTED_TWICE: &str =
    include_str!("container_secrets_mounted_twice.mountinfo");

fn aliases_of(text: &str, store: (u64, &str)) -> Result<Vec<Alias>, MountDoubt> {
    Table::of(text.as_bytes())?.aliases(&mount(text, store.0)?, &at(text, store.0, store.1)?)
}

fn points(aliases: &[Alias]) -> Vec<String> {
    aliases
        .iter()
        .map(|alias| String::from_utf8_lossy(&alias.mount_point).into_owned())
        .collect()
}

/// X5 final review (P3): the secrets volume mounted again, or a folder of it, is found
/// wherever it is mounted, with where the store is reached through it; the store's own
/// mount, the data volume and a container's files are not such mounts.
#[test]
fn another_mount_of_the_release_directory_is_found_wherever_it_is() {
    let aliases = aliases_of(CONTAINER_SECRETS_MOUNTED_TWICE, (226, "/secrets")).unwrap();
    assert_eq!(points(&aliases), ["/data/extra", "/data/instance-keys"]);
    assert!(aliases.iter().all(|alias| alias.within(b"/data").unwrap()));
    assert!(aliases[0].within(b"/data/extra").unwrap());
    assert!(!aliases[0].within(b"/data/instance").unwrap());
    assert!(!aliases[1].within(b"/data/instance").unwrap());
    assert_eq!(aliases[0].within(b"relative"), Err(MountDoubt::Malformed));
    // The data volume shows no directory of the secrets volume: two volumes of one
    // filesystem, apart.
    assert!(
        aliases_of(CONTAINER_SECRETS_MOUNTED_TWICE, (225, "/data/instance"))
            .unwrap()
            .is_empty()
    );
    // One mount of each volume: none. A folder of the data volume mounted inside the
    // secrets volume is not the secrets volume's (`mounted_inside` finds that one).
    for (text, store) in [
        (CONTAINER_TWO_VOLUMES, 106),
        (CONTAINER_MOUNT_INSIDE_SECRETS, 185),
    ] {
        assert!(aliases_of(text, (store, "/secrets")).unwrap().is_empty());
    }
    // A secrets folder of the data volume: the data volume's mount contains it.
    let inside = aliases_of(CONTAINER_SECRETS_INSIDE_DATA, (106, "/secrets")).unwrap();
    assert_eq!(points(&inside), ["/data"]);
    assert!(inside[0].within(b"/data/secrets").unwrap());
}

/// The same question on an ordinary machine: a store on the root filesystem, which is
/// mounted at `/` and nowhere else unless something says so.
#[test]
fn another_mount_of_a_store_on_the_disk_is_found_with_where_it_shows_it() {
    let store = (27, "/home/a/.local/state/agentic-org/credentials/i");
    // Nothing else of the disk, another filesystem, a mount of a namespace file, a
    // deleted root, a mount over `/` itself, and a folder of the disk beside the
    // store: none.
    let plain = table(&[
        "48 27 254:0 /var/lib/docker /var/lib/docker rw - ext4 /dev/vda rw",
        "57 27 0:4 net:[4026532268] /run/docker/netns/1c rw - nsfs nsfs rw",
        "58 27 254:0 /home/a/.local/state/agentic-org/credentials/i/gone//deleted /mnt/gone rw - ext4 /dev/vda rw",
        "59 1 254:0 / / rw - ext4 /dev/vda rw",
        "60 27 254:16 /home/a/.local/state/agentic-org/credentials/i /mnt/other-disk rw - ext4 /dev/vdb rw",
    ]);
    assert!(aliases_of(&plain, store).unwrap().is_empty());
    // The disk's top mounted again (`/sysroot`, as on an image-based system), the
    // store bound into the data root, and a folder of it bound elsewhere.
    let text = table(&[
        "70 27 254:0 / /sysroot rw - ext4 /dev/vda rw",
        "71 27 254:0 /home/a/.local/state/agentic-org/credentials/i /home/a/data/keys rw - ext4 /dev/vda rw",
        r"72 27 254:0 /home/a/.local/state/agentic-org/credentials/i/office-keys /mnt/k\040eys rw - ext4 /dev/vda rw",
        // A folder named as a deleted root would be by the older mark: still a mount of
        // the store, and found.
        r"73 27 254:0 /home/a/.local/state/agentic-org/credentials/i/x\040(deleted) /mnt/x rw - ext4 /dev/vda rw",
    ]);
    let aliases = aliases_of(&text, store).unwrap();
    assert_eq!(
        points(&aliases),
        ["/sysroot", "/home/a/data/keys", "/mnt/k eys", "/mnt/x"]
    );
    assert_eq!(
        aliases[0].shows,
        [
            &b"sysroot"[..],
            b"home",
            b"a",
            b".local",
            b"state",
            b"agentic-org",
            b"credentials",
            b"i"
        ]
    );
    assert!(!aliases[0].within(b"/home/a/data").unwrap());
    assert!(aliases[1].within(b"/home/a/data").unwrap());
    assert!(aliases[1].within(b"/home/a/data/keys").unwrap());
    assert!(!aliases[2].within(b"/home/a/data").unwrap());
    // A line whose device, root or mount point cannot be read could be one.
    for line in [
        "73 27 254 / /x rw - ext4 /dev/vda rw",
        "73 27 254:0 /a\\b /x rw - ext4 /dev/vda rw",
        "73 27 254:0 / relative rw - ext4 /dev/vda rw",
        "73 27 254:0 /a//b /x rw - ext4 /dev/vda rw",
        "73 27",
    ] {
        assert_eq!(
            aliases_of(&table(&[line]), store),
            Err(MountDoubt::Malformed),
            "{line}"
        );
    }
}

/// X5 review (third round): a mount inside the release directory, at any depth, is
/// found by its mount point, however the volumes share a filesystem; the directory's
/// own mount, a mount beside it with a longer name, and mounts elsewhere are not.
#[test]
fn a_mount_inside_the_release_directory_is_found_by_its_mount_point() {
    let nested = Table::of(CONTAINER_MOUNT_INSIDE_SECRETS.as_bytes()).unwrap();
    assert_eq!(
        nested.mounted_inside(b"/secrets").unwrap().as_deref(),
        Some(&b"/secrets/dev-keys"[..])
    );
    assert_eq!(nested.mounted_inside(b"/data").unwrap(), None);
    assert_eq!(nested.mounted_inside(b"/secrets/dev-keys").unwrap(), None);
    for table in [CONTAINER_TWO_VOLUMES, CONTAINER_SECRETS_INSIDE_DATA] {
        let table = Table::of(table.as_bytes()).unwrap();
        assert_eq!(table.mounted_inside(b"/secrets").unwrap(), None);
    }
    let line = |id: u32, point: &str| format!("{id} 1 8:1 /x {point} rw - ext4 /dev/sda1 rw\n");
    let table = [
        line(1, "/"),
        line(2, "/secrets"),
        line(3, "/secrets-old"),
        line(4, "/secrets/office-keys/a\\040b"),
    ]
    .concat();
    assert_eq!(
        Table::of(table.as_bytes())
            .unwrap()
            .mounted_inside(b"/secrets")
            .unwrap()
            .as_deref(),
        Some(&b"/secrets/office-keys/a b"[..])
    );
    // A line whose mount point cannot be read could be one.
    let unreadable = [line(1, "/"), "2 1 8:1 /x relative rw - ext4 s rw\n".into()].concat();
    assert_eq!(
        Table::of(unreadable.as_bytes())
            .unwrap()
            .mounted_inside(b"/secrets"),
        Err(MountDoubt::Malformed)
    );
    assert_eq!(
        nested.mounted_inside(b"relative"),
        Err(MountDoubt::Malformed)
    );
}

/// A mount table as the kernel writes one: the root filesystem, `/proc`, a
/// separate filesystem for `/srv`, and whatever a test adds.
fn table(more: &[&str]) -> String {
    let mut text = String::from(
        "22 27 0:21 / /proc rw,relatime - proc proc rw\n\
         27 1 254:0 / / rw,relatime - ext4 /dev/vda rw,discard\n\
         40 27 254:16 / /srv rw,relatime shared:5 master:2 - ext4 /dev/vdb rw\n",
    );
    for line in more {
        text.push_str(line);
        text.push('\n');
    }
    text
}

fn mount(text: &str, mount_id: u64) -> Result<Mount, MountDoubt> {
    Table::of(text.as_bytes())?.mount(mount_id)
}

fn at(text: &str, mount_id: u64, path: &str) -> Result<Place, MountDoubt> {
    place(&mount(text, mount_id)?, path.as_bytes())
}

fn judged(text: &str, store: (u64, &str), data_root: (u64, &str)) -> Result<(), RootRule> {
    disjoint(
        &at(text, store.0, store.1).unwrap(),
        &at(text, data_root.0, data_root.1).unwrap(),
    )
}

/// The store against what the data root's mount shows, as a container's host asks.
fn volumes(text: &str, store: (u64, &str), data_mount: u64) -> Result<(), RootRule> {
    separate_volumes(
        &at(text, store.0, store.1).unwrap(),
        &shown(&mount(text, data_mount).unwrap()).unwrap(),
    )
}

fn names_of(place: Place) -> Vec<String> {
    place
        .path
        .iter()
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

// -----------------------------------------------------------------------------
// The index: every line's mount id, and nothing else of a line.
// -----------------------------------------------------------------------------

/// Lines the kernel really writes that are not mounts of a directory, and lines
/// that are not of its form at all: none is one of the two mounts looked for, so
/// none is read beyond its id, and the mounts that are looked for are found.
#[test]
fn a_line_that_is_not_looked_for_never_refuses_the_table() {
    let foreign = [
        // Namespace files (Docker, `ip netns add`, `unshare --mount=<file>`).
        "57 27 0:4 net:[4026532268] /run/docker/netns/1c0bb8fe139b rw - nsfs nsfs rw",
        "58 27 0:4 mnt:[4026532841] /run/snapd/ns/lxd.mnt rw - nsfs nsfs rw",
        "59 27 0:4 pid:[4026532842] /run/pidns/keep rw shared:9 - nsfs nsfs rw",
        // A file bound over a file, as every container has three of.
        "60 27 254:0 /var/lib/docker/containers/abc/resolv.conf /etc/resolv.conf rw,relatime - ext4 /dev/vda rw",
        // An overlay with options full of colons and commas.
        "61 27 0:40 / /var/lib/docker/overlay2/x/merged rw,relatime shared:2 - overlay overlay rw,lowerdir=/a/1:/a/2:/a/3,upperdir=/a/4/diff,workdir=/a/4/work,index=off,xino=off",
        // autofs, FUSE with a source that has spaces, cgroup2, a source of `none`,
        // and an empty source (two spaces).
        "62 27 0:41 / /net rw,relatime shared:3 - autofs /etc/auto.net rw,fd=6,pgrp=1,timeout=300,minproto=5,maxproto=5,indirect",
        r"63 27 0:42 / /home/a/My\040Drive rw,nosuid,nodev,relatime - fuse.rclone remote:my\040files\040and\040more rw,user_id=1000,group_id=1000",
        "64 23 0:38 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:4 - cgroup2 cgroup2 rw,nsdelegate,memory_recursiveprot",
        "65 27 0:43 / /mnt/none rw - tmpfs none rw",
        "66 27 0:44 / /mnt/empty rw - tmpfs  rw",
        // Optional fields: none above, one, and several.
        "67 27 254:0 /a /mnt/a rw master:1 - ext4 /dev/vda rw",
        "68 27 254:0 /b /mnt/b rw shared:1 master:2 propagate_from:3 unbindable - ext4 /dev/vda rw",
        // Not the kernel's form, with a mount id that can be read all the same.
        "70 1 254:0 / / rw,relatime ext4 /dev/vda rw",
        "71 1 254:0 / / - ext4 /dev/vda rw",
        "72 1 254 / / rw - ext4 /dev/vda rw",
        "73 x y z",
        "74",
        "75 1 254:0 data relative rw - ext4 /dev/vda rw",
        "76 1 254:0 /a\\04 / rw - ext4 /dev/vda rw",
        "77 1 254:0 /a\0b / rw - ext4 /dev/vda rw",
        "78 -1 4294967296:0 / / rw - ext4 /dev/vda rw",
        "79 1 254:0  / / rw - ext4 /dev/vda rw",
        "80 ",
    ];
    let text = table(&foreign);
    let index = Table::of(text.as_bytes()).unwrap();
    assert_eq!(index.len(), 3 + foreign.len());
    // The mounts that are looked for are found, and placed, among all of that.
    assert_eq!(
        names_of(at(&text, 27, "/home/a/x").unwrap()),
        ["home", "a", "x"]
    );
    assert_eq!(judged(&text, (40, "/srv/secrets"), (27, "/home/a")), Ok(()));
    assert_eq!(
        judged(&text, (27, "/home/a/data/secrets"), (27, "/home/a/data")),
        Err(RootRule::InsideDataRoot)
    );
}

/// A line whose mount id cannot be read could be the line of one of the two
/// mounts looked for, or a second line for one of them: the table is refused.
#[test]
fn a_line_whose_mount_id_cannot_be_read_refuses_the_table() {
    let good = "27 1 254:0 / / rw,relatime - ext4 /dev/vda rw\n";
    assert_eq!(Table::of(good.as_bytes()).unwrap().len(), 1);
    for unreadable in [
        // No number where the id belongs.
        "x 1 254:0 / / rw - ext4 /dev/vda rw\n",
        "+27 1 254:0 / / rw - ext4 /dev/vda rw\n",
        "-27 1 254:0 / / rw - ext4 /dev/vda rw\n",
        "0x1b 1 254:0 / / rw - ext4 /dev/vda rw\n",
        "27\t1 254:0 / / rw - ext4 /dev/vda rw\n",
        " 27 1 254:0 / / rw - ext4 /dev/vda rw\n",
        "99999999999999999999999 1 254:0 / / rw - ext4 /dev/vda rw\n",
        "net:[4026532268] 27 0:4 / /run rw - nsfs nsfs rw\n",
        // An empty line.
        "\n",
        // Cut off: no newline at the end, in the middle of a line or of a number.
        "27 1 254:0 / / rw,relatime - ext4 /dev/vda rw",
        "27 1 254:0 / /",
        "2",
    ] {
        assert_eq!(
            Table::of(unreadable.as_bytes()).err(),
            Some(MountDoubt::Malformed),
            "{unreadable:?}"
        );
        // One such line spoils a table of good ones, wherever it is.
        for mixed in [format!("{good}{unreadable}"), format!("{unreadable}{good}")] {
            if unreadable.ends_with('\n') {
                assert_eq!(
                    Table::of(mixed.as_bytes()).err(),
                    Some(MountDoubt::Malformed),
                    "{mixed:?}"
                );
            }
        }
    }
    // An empty table is a table without a mount.
    assert_eq!(Table::of(b"").unwrap().len(), 0);
    assert_eq!(mount("", 27), Err(MountDoubt::NotListed));
    // A table that is too large to have been read whole.
    let mut oversized = good.as_bytes().to_vec();
    oversized.resize(LARGEST_TABLE + 1, b'x');
    assert_eq!(Table::of(&oversized).err(), Some(MountDoubt::Malformed));
}

// -----------------------------------------------------------------------------
// The line of a mount that is looked for.
// -----------------------------------------------------------------------------

#[test]
fn the_line_of_a_mount_that_is_looked_for_is_read_exactly() {
    let text = table(&[
        // Optional fields, none and several; an empty source; a source of `none`.
        "61 27 254:0 /home/a/agentic /data rw,relatime - ext4 /dev/vda rw",
        "62 27 254:0 /home/a/agentic/secrets /secrets rw shared:1 master:2 propagate_from:3 unbindable - ext4 /dev/vda rw",
        "63 27 0:44 / /run/x rw - tmpfs  rw",
        "65 27 0:45 / /run/y rw master:7 - tmpfs none rw",
        // A space, a tab, a newline and a backslash in a path, as the kernel writes them.
        r"64 27 254:0 /with\040space/and\011tab /mnt/new\012line\134slash rw - ext4 /dev/vda rw",
        // A source with spaces, and options with colons and commas, are not read.
        r"66 27 0:46 / /home/a/My\040Drive rw - fuse.rclone remote:my\040files rw,user_id=1000",
        "67 27 0:47 / /merged rw shared:2 - overlay overlay rw,lowerdir=/a/1:/a/2,upperdir=/a/3 - /x,workdir=/a/w",
    ]);
    assert_eq!(
        mount(&text, 27),
        Ok(Mount {
            filesystem: (254, 0),
            root: b"/".to_vec(),
            mount_point: b"/".to_vec(),
        })
    );
    assert_eq!(mount(&text, 40).unwrap().filesystem, (254, 16));
    assert_eq!(mount(&text, 61).unwrap().root, b"/home/a/agentic");
    assert_eq!(mount(&text, 62).unwrap().mount_point, b"/secrets");
    assert_eq!(mount(&text, 63).unwrap().filesystem, (0, 44));
    assert_eq!(mount(&text, 65).unwrap().mount_point, b"/run/y");
    let escaped = mount(&text, 64).unwrap();
    assert_eq!(escaped.root, b"/with space/and\ttab");
    assert_eq!(escaped.mount_point, b"/mnt/new\nline\\slash");
    assert_eq!(mount(&text, 66).unwrap().mount_point, b"/home/a/My Drive");
    assert_eq!(mount(&text, 67).unwrap().mount_point, b"/merged");
    // A path cannot fake a separator or a second line: the kernel escapes both.
    let faked = "27 1 254:0 /a\\040-\\040ext4 /b\\01228\\0401 rw - ext4 /dev/vda rw\n";
    assert_eq!(Table::of(faked.as_bytes()).unwrap().len(), 1);
    let faked = mount(faked, 27).unwrap();
    assert_eq!(faked.root, b"/a - ext4");
    assert_eq!(faked.mount_point, b"/b\n28 1");
}

/// The same hostile shapes that are skipped in another mount's line are a doubt
/// in the line of a mount that is looked for: nothing is guessed about it.
#[test]
fn a_line_that_is_looked_for_and_is_not_the_kernels_form_is_a_doubt() {
    for hostile in [
        // No separator, the separator too early, or nothing after it.
        "90 1 254:0 / / rw,relatime ext4 /dev/vda rw",
        "90 1 254:0 / / - ext4 /dev/vda rw",
        "90 1 254:0 / / rw,relatime -",
        "90 1 254:0 / / rw,relatime - ",
        // Too few fields.
        "90",
        "90 ",
        "90 1 254:0 /",
        // Fields that are not numbers, or more than numbers.
        "90 x 254:0 / / rw - ext4 /dev/vda rw",
        "90 -1 254:0 / / rw - ext4 /dev/vda rw",
        "90 1 254 / / rw - ext4 /dev/vda rw",
        "90 1 254:0:1 / / rw - ext4 /dev/vda rw",
        "90 1 254:x / / rw - ext4 /dev/vda rw",
        "90 1 4294967296:0 / / rw - ext4 /dev/vda rw",
        // A mount point that is no absolute path.
        "90 1 254:0 / data rw - ext4 /dev/vda rw",
        // Two spaces: an empty field among the fixed ones.
        "90 1 254:0  / / rw - ext4 /dev/vda rw",
        "90  1 254:0 / / rw - ext4 /dev/vda rw",
        // Escapes the kernel does not write: too short, not octal, too large, bare.
        "90 1 254:0 /a\\04 / rw - ext4 /dev/vda rw",
        "90 1 254:0 /a\\ / rw - ext4 /dev/vda rw",
        "90 1 254:0 /a\\089 / rw - ext4 /dev/vda rw",
        "90 1 254:0 /a\\777 / rw - ext4 /dev/vda rw",
        "90 1 254:0 /a\\\\ / rw - ext4 /dev/vda rw",
        "90 1 254:0 / /a\\x41 rw - ext4 /dev/vda rw",
        // A tab where a space belongs, and a NUL byte.
        "90 1 254:0\t/ / rw - ext4 /dev/vda rw",
        "90 1 254:0 /a\0b / rw - ext4 /dev/vda rw",
    ] {
        let text = table(&[hostile]);
        assert_eq!(mount(&text, 90), Err(MountDoubt::Malformed), "{hostile:?}");
        // The other mounts of the same table are still found.
        assert!(mount(&text, 27).is_ok(), "{hostile:?}");
    }
    let text = table(&[
        // A root that is no path: the mount of a namespace file.
        "91 27 0:4 net:[4026532268] /run/docker/netns/1c0bb8fe139b rw - nsfs nsfs rw",
        "92 27 254:0 data /mnt rw - ext4 /dev/vda rw",
        // The same id twice.
        "93 27 254:0 /x /one rw - ext4 /dev/vda rw",
        "93 27 254:0 /y /two rw - ext4 /dev/vda rw",
        // Twice, and one of the two is not even a mount's line.
        "94 27 254:0 /x /one rw - ext4 /dev/vda rw",
        "94",
        // Mounts whose root directory was deleted, as the kernel marks them.
        "95 27 254:0 /home/a/gone//deleted /gone rw - ext4 /dev/vda rw",
        r"96 27 254:0 /home/a/gone\040(deleted) /gone2 rw - ext4 /dev/vda rw",
    ]);
    assert_eq!(mount(&text, 91), Err(MountDoubt::RootNotAPath));
    assert_eq!(mount(&text, 92), Err(MountDoubt::RootNotAPath));
    assert_eq!(mount(&text, 93), Err(MountDoubt::Ambiguous));
    assert_eq!(mount(&text, 94), Err(MountDoubt::Ambiguous));
    assert_eq!(mount(&text, 95), Err(MountDoubt::DeletedRoot));
    assert_eq!(mount(&text, 96), Err(MountDoubt::DeletedRoot));
    assert_eq!(mount(&text, 99), Err(MountDoubt::NotListed));
}

// -----------------------------------------------------------------------------
// Places.
// -----------------------------------------------------------------------------

#[test]
fn a_place_is_the_mounts_root_and_the_path_below_the_mount_point() {
    let text = table(&[
        "61 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
        r"64 27 254:0 /with\040space /mnt/new\012line rw - ext4 /dev/vda rw",
    ]);
    assert_eq!(names_of(at(&text, 27, "/").unwrap()), [""; 0]);
    assert_eq!(
        names_of(at(&text, 27, "/home/a/x").unwrap()),
        ["home", "a", "x"]
    );
    assert_eq!(
        names_of(at(&text, 61, "/data").unwrap()),
        ["home", "a", "agentic"]
    );
    assert_eq!(
        names_of(at(&text, 61, "/data/office-state/offices").unwrap()),
        ["home", "a", "agentic", "office-state", "offices"]
    );
    assert_eq!(names_of(at(&text, 40, "/srv/x").unwrap()), ["x"]);
    assert_eq!(at(&text, 40, "/srv/x").unwrap().filesystem, (254, 16));
    assert_eq!(
        names_of(at(&text, 64, "/mnt/new\nline/below it").unwrap()),
        ["with space", "below it"]
    );
    // What a mount shows is its root.
    assert_eq!(
        names_of(shown(&mount(&text, 61).unwrap()).unwrap()),
        ["home", "a", "agentic"]
    );
    assert_eq!(
        names_of(shown(&mount(&text, 27).unwrap()).unwrap()),
        [""; 0]
    );
    // The mount point is a prefix by whole names only.
    assert_eq!(
        at(&text, 61, "/database"),
        Err(MountDoubt::PathOutsideMount)
    );
}

#[test]
fn every_doubt_about_a_directorys_path_is_reported_and_none_is_a_place() {
    let text = table(&[
        "61 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
        // Paths the kernel does not write.
        "73 27 254:0 /a/../b /dots rw - ext4 /dev/vda rw",
        "74 27 254:0 /a /dots/./x rw - ext4 /dev/vda rw",
        "75 27 254:0 /a//b /slashes rw - ext4 /dev/vda rw",
    ]);
    assert_eq!(at(&text, 73, "/dots"), Err(MountDoubt::Malformed));
    assert_eq!(at(&text, 74, "/dots/./x"), Err(MountDoubt::Malformed));
    assert_eq!(at(&text, 75, "/slashes"), Err(MountDoubt::Malformed));
    assert_eq!(
        shown(&mount(&text, 73).unwrap()),
        Err(MountDoubt::Malformed)
    );
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
            at(&text, 61, path),
            Err(MountDoubt::PathOutsideMount),
            "{path:?}"
        );
    }
}

/// The reviewer's case and its reverse: one of the two is a bind mount of a
/// directory inside the other. The walk through `..` cannot see it; the places do.
#[test]
fn a_bind_mount_of_a_directory_inside_the_other_is_found_by_its_place() {
    let text = table(&[
        // `mount --bind data/inner alias2`, both below `/work` on the root filesystem.
        "80 27 254:0 /work/data/inner /work/alias2 rw - ext4 /dev/vda rw",
        // `docker run -v ./agentic:/data -v ./agentic/secrets:/secrets`.
        "81 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
        "82 27 254:0 /home/a/agentic/secrets /secrets rw - ext4 /dev/vda rw",
    ]);
    // The store is a bind of a directory of the data root.
    assert_eq!(
        judged(&text, (80, "/work/alias2"), (27, "/work/data")),
        Err(RootRule::InsideDataRoot)
    );
    assert_eq!(
        judged(&text, (82, "/secrets"), (81, "/data")),
        Err(RootRule::InsideDataRoot)
    );
    // A directory below such a bind is inside the data root as well.
    assert_eq!(
        judged(&text, (80, "/work/alias2/deeper"), (27, "/work/data")),
        Err(RootRule::InsideDataRoot)
    );
    // The reverse: the data root is a bind of a directory of the store.
    assert_eq!(
        judged(&text, (27, "/work/data"), (80, "/work/alias2")),
        Err(RootRule::ContainsDataRoot)
    );
    assert_eq!(
        judged(&text, (81, "/data"), (82, "/secrets")),
        Err(RootRule::ContainsDataRoot)
    );
}

/// The two cases the walk through `..` already refuses are refused by their places
/// too, so the two checks never disagree about them.
#[test]
fn the_same_directory_through_a_bind_and_a_store_below_a_bind_alias_are_refused() {
    let text = table(&[
        "83 27 254:0 /work/store /work/store-again rw - ext4 /dev/vda rw",
        "84 27 254:0 /work/data /work/data-alias rw - ext4 /dev/vda rw",
    ]);
    // The same directory twice, once through a bind.
    assert_eq!(
        judged(&text, (27, "/work/store"), (83, "/work/store-again")),
        Err(RootRule::IsDataRoot)
    );
    assert_eq!(
        judged(&text, (83, "/work/store-again"), (83, "/work/store-again")),
        Err(RootRule::IsDataRoot)
    );
    // A store inside a data root that was given through a bind alias.
    assert_eq!(
        judged(
            &text,
            (27, "/work/data/credentials"),
            (84, "/work/data-alias")
        ),
        Err(RootRule::InsideDataRoot)
    );
    assert_eq!(
        judged(
            &text,
            (84, "/work/data-alias/credentials"),
            (27, "/work/data")
        ),
        Err(RootRule::InsideDataRoot)
    );
}

#[test]
fn directories_that_share_nothing_are_accepted() {
    let text = table(&[
        // Two named volumes of one disk, as a container sees them.
        "85 27 254:0 /var/lib/docker/volumes/data/_data /data rw - ext4 /dev/vda rw",
        "86 27 254:0 /var/lib/docker/volumes/secrets/_data /secrets rw - ext4 /dev/vda rw",
        // A name that only starts like the other.
        "87 27 254:0 /work/data-2 /elsewhere rw - ext4 /dev/vda rw",
    ]);
    assert_eq!(judged(&text, (86, "/secrets"), (85, "/data")), Ok(()));
    assert_eq!(
        judged(
            &text,
            (27, "/home/a/.local/state/x"),
            (27, "/home/a/.local/share/x")
        ),
        Ok(())
    );
    assert_eq!(
        judged(&text, (87, "/elsewhere"), (27, "/work/data")),
        Ok(())
    );
    assert_eq!(
        judged(&text, (27, "/work/data"), (87, "/elsewhere")),
        Ok(())
    );
    // Another filesystem shares no directory, whatever the paths are: the same
    // path on two devices, and a filesystem mounted below the data root. (The
    // second is refused all the same, by the walk through `..`: `rules::disjoint`.)
    assert_eq!(judged(&text, (40, "/srv"), (27, "/")), Ok(()));
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
    let text = "27 1 0:30 /@ / rw - btrfs /dev/vda2 rw,subvol=/@\n\
                90 27 0:30 /@data /data rw - btrfs /dev/vda2 rw,subvol=/@data\n\
                91 27 0:30 /@data/secrets /secrets rw - btrfs /dev/vda2 rw,subvol=/@data/secrets\n\
                92 27 0:30 /@secrets /secrets2 rw - btrfs /dev/vda2 rw,subvol=/@secrets\n";
    assert_eq!(
        judged(text, (91, "/secrets"), (90, "/data")),
        Err(RootRule::InsideDataRoot)
    );
    assert_eq!(
        judged(text, (90, "/data"), (91, "/secrets")),
        Err(RootRule::ContainsDataRoot)
    );
    assert_eq!(judged(text, (92, "/secrets2"), (90, "/data")), Ok(()));
    assert_eq!(judged(text, (92, "/secrets2"), (27, "/home/a")), Ok(()));
}

/// A mount over another at the same mount point: each directory is placed by the
/// mount the kernel says it is in, so which of the two a path reaches today does
/// not matter. `/` mounted twice, and a volume mounted over a volume.
#[test]
fn a_directory_is_placed_by_its_own_mount_whatever_was_mounted_over_it() {
    let text = "1 0 0:2 / / rw - rootfs rootfs rw\n\
                27 1 254:0 / / rw,relatime - ext4 /dev/vda rw\n\
                30 27 254:0 /volumes/old /data rw - ext4 /dev/vda rw\n\
                31 27 254:0 /volumes/new /data rw - ext4 /dev/vda rw\n\
                32 27 254:0 /volumes/new/secrets /secrets rw - ext4 /dev/vda rw\n";
    assert_eq!(at(text, 1, "/x").unwrap().filesystem, (0, 2));
    assert_eq!(at(text, 27, "/x").unwrap().filesystem, (254, 0));
    assert_eq!(names_of(at(text, 30, "/data").unwrap()), ["volumes", "old"]);
    assert_eq!(names_of(at(text, 31, "/data").unwrap()), ["volumes", "new"]);
    // The store is inside the volume that is mounted last, and not inside the one
    // it covers: a host that holds a descriptor in either is judged by that one.
    assert_eq!(
        judged(text, (32, "/secrets"), (31, "/data")),
        Err(RootRule::InsideDataRoot)
    );
    assert_eq!(judged(text, (32, "/secrets"), (30, "/data")), Ok(()));
}

// -----------------------------------------------------------------------------
// Two volumes: the store against what the data root's mount shows.
// -----------------------------------------------------------------------------

/// The container's layout: the data volume at `/data`, the instance's root at
/// `/data/instance`, the store at `/secrets`.
#[test]
fn a_store_that_must_be_a_volume_is_not_in_the_data_volume_nor_around_it() {
    let text = table(&[
        // Two named volumes.
        "85 27 254:0 /var/lib/docker/volumes/data/_data /data rw - ext4 /dev/vda rw",
        "86 27 254:0 /var/lib/docker/volumes/secrets/_data /secrets rw - ext4 /dev/vda rw",
        // `-v D:/data -v D/secrets:/secrets`.
        "81 27 254:0 /home/a/agentic /data rw - ext4 /dev/vda rw",
        "82 27 254:0 /home/a/agentic/secrets /secrets rw - ext4 /dev/vda rw",
        // `-v D/secrets:/secrets -v D/secrets/data:/data`: the data in the secrets.
        "83 27 254:0 /home/b/secrets /secrets rw - ext4 /dev/vda rw",
        "84 27 254:0 /home/b/secrets/data /data rw - ext4 /dev/vda rw",
        // The same directory as both volumes.
        "88 27 254:0 /home/c/one /secrets rw - ext4 /dev/vda rw",
        "89 27 254:0 /home/c/one /data rw - ext4 /dev/vda rw",
    ]);
    // Two volumes: accepted by both rules.
    assert_eq!(
        judged(&text, (86, "/secrets"), (85, "/data/instance")),
        Ok(())
    );
    assert_eq!(volumes(&text, (86, "/secrets"), 85), Ok(()));
    // The store beside the instance root, inside the data volume: the walk and
    // the comparison with the data root itself both pass, the volumes do not.
    assert_eq!(
        judged(&text, (82, "/secrets"), (81, "/data/instance")),
        Ok(())
    );
    assert_eq!(
        volumes(&text, (82, "/secrets"), 81),
        Err(RootRule::InsideDataVolume)
    );
    // With the data root at the top of its volume, the first rule sees it too.
    assert_eq!(
        judged(&text, (82, "/secrets"), (81, "/data")),
        Err(RootRule::InsideDataRoot)
    );
    // The data volume inside the secrets volume, beside the items.
    assert_eq!(
        judged(&text, (83, "/secrets"), (84, "/data/instance")),
        Err(RootRule::ContainsDataRoot)
    );
    assert_eq!(
        volumes(&text, (83, "/secrets"), 84),
        Err(RootRule::ContainsDataVolume)
    );
    // One directory as both volumes.
    assert_eq!(
        judged(&text, (88, "/secrets"), (89, "/data/instance")),
        Err(RootRule::ContainsDataRoot)
    );
    assert_eq!(
        volumes(&text, (88, "/secrets"), 89),
        Err(RootRule::InsideDataVolume)
    );
    // Another filesystem is another volume, wherever it is mounted.
    assert_eq!(volumes(&text, (40, "/srv"), 81), Ok(()));
    // An ordinary machine: the data root's mount is the disk's own, which shows
    // everything on that disk. Every store on the same disk is inside that, which
    // is why the rule is asked only where a mount point is required.
    assert_eq!(
        judged(
            &text,
            (27, "/home/a/.local/state/agentic-org/credentials/x"),
            (27, "/home/a/.local/share/agentic-org/instances/default")
        ),
        Ok(())
    );
    assert_eq!(
        volumes(
            &text,
            (27, "/home/a/.local/state/agentic-org/credentials/x"),
            27
        ),
        Err(RootRule::InsideDataVolume)
    );
}

// -----------------------------------------------------------------------------
// Real tables, exactly as the kernel wrote them.
// -----------------------------------------------------------------------------

/// This machine with a Docker daemon and a container: the table that refused every
/// store when a line that is not a directory's mount refused the whole table.
#[test]
fn a_machine_that_runs_docker_has_a_table_that_places_a_store() {
    let index = Table::of(DOCKER_HOST.as_bytes()).unwrap();
    assert_eq!(index.len(), 29);
    assert!(DOCKER_HOST
        .contains("57 27 0:4 net:[4026532268] /run/docker/netns/1c0bb8fe139b rw - nsfs nsfs rw\n"));
    // A store and a data root on the root filesystem, as a host's account has them.
    assert_eq!(
        judged(
            DOCKER_HOST,
            (27, "/home/a/.local/state/agentic-org/credentials/0197"),
            (27, "/home/a/.local/share/agentic-org/instances/default")
        ),
        Ok(())
    );
    assert_eq!(
        judged(
            DOCKER_HOST,
            (27, "/home/a/data/secrets"),
            (27, "/home/a/data")
        ),
        Err(RootRule::InsideDataRoot)
    );
    // Docker's own directory is the root filesystem's `/var/lib/docker` bound on
    // itself: a directory reached through that mount is where it is on the disk.
    assert_eq!(
        judged(
            DOCKER_HOST,
            (48, "/var/lib/docker/volumes/s/_data"),
            (27, "/var/lib/docker/volumes")
        ),
        Err(RootRule::InsideDataRoot)
    );
    // The network namespace's mount shows no directory; the overlay's line, with
    // more than a thousand bytes of options, is a mount like any other.
    assert_eq!(index.mount(57), Err(MountDoubt::RootNotAPath));
    let overlay = index.mount(56).unwrap();
    assert_eq!(overlay.filesystem, (0, 40));
    assert_eq!(overlay.root, b"/");
    // `/dev/shm` and `/dev/pts` are each mounted twice at the same place here:
    // two filesystems, each found by its own id.
    assert_eq!(index.mount(25).unwrap().mount_point, b"/dev/shm");
    assert_eq!(index.mount(35).unwrap().mount_point, b"/dev/shm");
    assert_ne!(
        index.mount(25).unwrap().filesystem,
        index.mount(35).unwrap().filesystem
    );
    // Every line of it but the namespace's is a directory's mount.
    for line in DOCKER_HOST.lines() {
        let id: u64 = line.split(' ').next().unwrap().parse().unwrap();
        assert_eq!(index.mount(id).is_ok(), id != 57, "{line}");
    }
}

/// Inside a container with two named volumes: the store at `/secrets` and the
/// instance's root at `/data/instance` are two volumes of one disk.
#[test]
fn a_containers_table_with_two_volumes_accepts_the_store() {
    let index = Table::of(CONTAINER_TWO_VOLUMES.as_bytes()).unwrap();
    let (data, secrets) = (105, 106);
    assert_eq!(index.mount(data).unwrap().mount_point, b"/data");
    assert_eq!(index.mount(secrets).unwrap().mount_point, b"/secrets");
    assert_eq!(
        index.mount(secrets).unwrap().root,
        b"/var/lib/docker/volumes/x3probe-secrets/_data"
    );
    for data_root in ["/data", "/data/instance"] {
        assert_eq!(
            judged(
                CONTAINER_TWO_VOLUMES,
                (secrets, "/secrets"),
                (data, data_root)
            ),
            Ok(()),
            "{data_root}"
        );
    }
    assert_eq!(
        volumes(CONTAINER_TWO_VOLUMES, (secrets, "/secrets"), data),
        Ok(())
    );
    // The three files Docker binds into every container are mounts whose root is
    // a file. They are lines like any other, they are not directories' mounts,
    // and they are nowhere near the volumes.
    let mut files = 0;
    for line in CONTAINER_TWO_VOLUMES.lines() {
        let id: u64 = line.split(' ').next().unwrap().parse().unwrap();
        let found = index.mount(id).unwrap();
        if found.mount_point.starts_with(b"/etc/") {
            files += 1;
            assert!(found.root.starts_with(b"/var/lib/docker/containers/"));
            let file = place(&found, &found.mount_point).unwrap();
            let store = at(CONTAINER_TWO_VOLUMES, secrets, "/secrets").unwrap();
            assert_eq!(file.filesystem, store.filesystem);
            assert_eq!(disjoint(&store, &file), Ok(()));
            assert_eq!(separate_volumes(&store, &file), Ok(()));
        }
    }
    assert_eq!(files, 3);
    // The container's own root is an overlay: another filesystem than the volumes.
    assert_ne!(
        at(CONTAINER_TWO_VOLUMES, 87, "/home").unwrap().filesystem,
        at(CONTAINER_TWO_VOLUMES, secrets, "/secrets")
            .unwrap()
            .filesystem
    );
}

/// Inside a container started with `-v D/agentic:/data -v D/agentic/secrets:/secrets`.
#[test]
fn a_containers_table_with_the_secrets_inside_the_data_volume_refuses_the_store() {
    let (data, secrets) = (105, 106);
    // With the data root at the top of the data volume, the store is inside it.
    assert_eq!(
        judged(
            CONTAINER_SECRETS_INSIDE_DATA,
            (secrets, "/secrets"),
            (data, "/data")
        ),
        Err(RootRule::InsideDataRoot)
    );
    // With the instance's root below it, the store is beside the instance root,
    // and inside what the data root's mount shows.
    assert_eq!(
        judged(
            CONTAINER_SECRETS_INSIDE_DATA,
            (secrets, "/secrets"),
            (data, "/data/instance")
        ),
        Ok(())
    );
    assert_eq!(
        volumes(CONTAINER_SECRETS_INSIDE_DATA, (secrets, "/secrets"), data),
        Err(RootRule::InsideDataVolume)
    );
}

/// A node with several thousand mounts (a Kubernetes node: an overlay, volumes, a
/// network namespace and three file binds for every container): the table is
/// indexed and two mounts are placed in a time and a space that do not matter.
#[test]
fn a_table_of_several_thousand_mounts_is_indexed_quickly() {
    let mut text = table(&[]);
    let layers =
        "/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/1234/fs:".repeat(12);
    let mut id = 1000;
    for pod in 0..1500 {
        for line in [
            format!("{id} 27 0:{} / /run/containerd/io.containerd.runtime.v2.task/k8s.io/{pod:064x}/rootfs rw,relatime shared:{pod} - overlay overlay rw,lowerdir={layers},upperdir=/u/{pod}/fs,workdir=/u/{pod}/work", 2000 + pod),
            format!("{} 27 0:4 net:[{}] /run/netns/cni-{pod:08x} rw shared:{pod} - nsfs nsfs rw", id + 1, 4026532000u64 + pod),
            format!("{} 27 254:0 /var/lib/kubelet/pods/{pod:032x}/volumes/kubernetes.io~empty-dir/data /var/lib/kubelet/pods/{pod:032x}/volumes/kubernetes.io~empty-dir/data rw,relatime shared:1 - ext4 /dev/vda rw", id + 2),
            format!("{} 27 0:{} / /var/lib/kubelet/pods/{pod:032x}/volumes/kubernetes.io~projected/kube-api-access rw,relatime shared:{pod} - tmpfs tmpfs rw,size=16k", id + 3, 100 + pod),
        ] {
            text.push_str(&line);
            text.push('\n');
        }
        id += 4;
    }
    text.push_str("9001 27 254:0 /var/lib/volumes/data /data rw - ext4 /dev/vda rw\n");
    text.push_str("9002 27 254:0 /var/lib/volumes/secrets /secrets rw - ext4 /dev/vda rw\n");
    let started = Instant::now();
    let index = Table::of(text.as_bytes()).unwrap();
    let store = place(&index.mount(9002).unwrap(), b"/secrets").unwrap();
    let data_mount = index.mount(9001).unwrap();
    let data_root = place(&data_mount, b"/data/instance").unwrap();
    assert_eq!(disjoint(&store, &data_root), Ok(()));
    assert_eq!(
        separate_volumes(&store, &shown(&data_mount).unwrap()),
        Ok(())
    );
    let elapsed = started.elapsed();
    assert_eq!(index.len(), 3 + 6000 + 2);
    println!(
        "{} mounts, {} bytes: indexed and two mounts placed in {elapsed:?}; the index is {} bytes",
        index.len(),
        text.len(),
        index.len() * std::mem::size_of::<(u64, &[u8])>()
    );
    // Generous: an unoptimized build on a busy machine does this in milliseconds.
    assert!(elapsed.as_secs() < 5, "{elapsed:?}");
    assert!(text.len() < LARGEST_TABLE / 4);
}
