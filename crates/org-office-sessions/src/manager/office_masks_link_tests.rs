//! Links and linked prefixes (`TASK-204`): a link a mask meets, as a neighbour, a masked
//! folder or a swept entry, is masked as the link itself, never through and never as what it
//! points to; the office's own roots are found through a linked parent (`/var` and
//! `/private/var` on macOS); and the masks come out of the Linux `bwrap` pruning of absent
//! paths as they went in.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use org_agent_runtime::{PathPattern, PermissionPolicy, SandboxSpec};

use super::super::spec::prune_nonexistent_bwrap_denies;
use super::tests::{
    assert_masked_whole, assert_reachable, denies, placed_masks, Host, HQ, ORG_A, ORG_B, SAT_1,
    SAT_2,
};
use super::{own_root_forms, MaskedFolder, OfficeMasks, Sweep};

/// Makes `at` a link to `to`.
fn link(at: PathBuf, to: &Path) -> PathBuf {
    symlink(to, &at).unwrap();
    at
}

/// `link` is masked exactly and never through.
fn assert_masked_as_link(denied: &[String], link: &Path) {
    let raw = link.display().to_string();
    assert!(denied.contains(&raw), "{raw} missing: {denied:?}");
    assert!(
        !denied.iter().any(|d| d.starts_with(&format!("{raw}/"))),
        "{raw} is masked through: {denied:?}"
    );
}

#[test]
fn a_linked_neighbour_or_folder_is_masked_as_the_link_never_through_nor_as_its_target() {
    let host = Host::new();
    let org = host.mk(ORG_A);
    let (durable, runtime) = (org.join("durable"), org.join("runtime"));
    let home = host.mk(&format!(
        "{ORG_A}/durable/local/offices/{HQ}/agents/me/home"
    ));
    let worktree = host.mk(&format!("{ORG_A}/runtime/{HQ}/teams/t1/agents/me/wt"));
    let outside = host.mk("outside");
    let secret = host.file("outside/secret");
    // Neighbours that are links: to the office's own organization, into its own home, to a
    // folder outside, nowhere, and `/`.
    let neighbours = [
        link(host.data.join("to-own-org"), &org),
        link(host.data.join("to-own-home"), &home),
        link(host.data.join("to-outside"), &outside),
        link(host.data.join("dangling"), &host.data.join("gone")),
        link(host.data.join("to-root"), Path::new("/")),
    ];
    // Durable families that are links: into the session's own home, outside, `/`, its org.
    let families = [
        link(durable.join("state"), &home),
        link(durable.join("journal"), &outside),
        link(durable.join("blobs"), Path::new("/")),
        link(durable.join("recovery"), &org),
    ];
    // Folders that hold the office's own roots: never masked, never made.
    let holders = [host.data.clone(), org.clone(), durable.clone()];
    let mut given = neighbours.to_vec();
    // `lstat("link/")` and `lstat("link/.")` follow the link: both are looked at as `link`.
    given.push(PathBuf::from(format!("{}/", neighbours[0].display())));
    given.push(PathBuf::from(format!("{}/.", neighbours[1].display())));
    let masks = OfficeMasks {
        folders: families
            .iter()
            .chain(&holders)
            .map(|path| MaskedFolder {
                path: path.clone(),
                pre_create: true,
            })
            .collect(),
        files: vec![link(org.join("hq-owner.v1.json"), &home)],
        neighbours: given,
        sweeps: Vec::new(),
        own_roots: own_root_forms([&durable, &runtime]),
    };

    let denied = denies(&masks);
    for link in neighbours.iter().chain(&families) {
        assert_masked_as_link(&denied, link);
    }
    assert_masked_as_link(&denied, &org.join("hq-owner.v1.json"));
    // Nothing a link points to is masked: the office's own trees, the folder outside, `/`.
    for path in [
        &org, &durable, &runtime, &home, &worktree, &outside, &secret,
    ] {
        assert_reachable(&denied, path);
    }
    let pre_created: Vec<&Path> = masks.pre_created().collect();
    for holder in &holders {
        assert!(
            !pre_created.contains(&holder.as_path()),
            "{} is made: {pre_created:?}",
            holder.display()
        );
    }
}

#[test]
fn own_roots_reached_through_a_linked_parent_are_held_by_either_prefix() {
    // `/var` is a link to `/private/var` on macOS: the roots may be given through the link and
    // a neighbour found through the real folder, or the other way round.
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path().canonicalize().unwrap();
    let real = tmp.join("private").join("var");
    for root in ["durable", "runtime"] {
        std::fs::create_dir_all(real.join(ORG_A).join(root)).unwrap();
    }
    std::fs::create_dir_all(real.join(ORG_B).join("durable")).unwrap();
    let linked = link(tmp.join("var"), &real);

    for (given, other) in [(&linked, &real), (&real, &linked)] {
        let own = given.join(ORG_A);
        let (durable, runtime) = (own.join("durable"), own.join("runtime"));
        let other_org = other.join(ORG_A);
        let masks = OfficeMasks {
            folders: Vec::new(),
            files: Vec::new(),
            neighbours: vec![
                other_org.clone(),
                other_org.join("durable"),
                PathBuf::from(format!("{}/", other_org.join("runtime").display())),
            ],
            // A sweep that keeps nothing, through the other prefix.
            sweeps: vec![Sweep::all_but(other, std::iter::empty())],
            own_roots: own_root_forms([&durable, &runtime]),
        };
        for prefix in [given, other] {
            let org = prefix.join(ORG_A);
            for holder in [
                org.clone(),
                org.join("durable"),
                org.join("runtime"),
                org.join("runtime/."),
            ] {
                assert!(
                    masks.holds_own_root(&holder),
                    "{} holds {}",
                    holder.display(),
                    durable.display()
                );
            }
            for unrelated in [
                prefix.join(ORG_B),
                org.join("offices"),
                org.join("durable").join("state"),
            ] {
                assert!(
                    !masks.holds_own_root(&unrelated),
                    "{} holds no root of {}",
                    unrelated.display(),
                    own.display()
                );
            }
        }
        let denied = denies(&masks);
        assert_masked_whole(&denied, &other.join(ORG_B));
        for prefix in [&real, &linked] {
            for root in ["durable", "runtime"] {
                assert_reachable(&denied, &prefix.join(ORG_A).join(root));
            }
        }
    }
}

#[test]
fn prune_nonexistent_bwrap_denies_keeps_links_as_links_and_never_reaches_the_offices_trees() {
    let host = Host::new();
    let own = host.mk(&format!("{ORG_A}/offices/{SAT_1}"));
    let (durable, runtime) = (own.join("durable"), own.join("runtime"));
    let home = host.mk(&format!(
        "{ORG_A}/offices/{SAT_1}/durable/local/offices/{SAT_1}/agents/me/home"
    ));
    let worktree = host.mk(&format!(
        "{ORG_A}/offices/{SAT_1}/runtime/{SAT_1}/teams/t1/agents/me/wt"
    ));
    host.mk(&format!("{ORG_B}/durable/state"));
    host.mk(&format!("{ORG_A}/offices/{SAT_2}/durable/state"));
    // Links where an organization's folder belongs (to the office's own organization, and
    // dangling), and a durable family that is a link to the session's own home.
    let links = [
        link(
            host.data.join("0195a000-0000-7000-8000-00000000c001"),
            &host.data.join(ORG_A),
        ),
        link(
            host.data.join("0195a000-0000-7000-8000-00000000c002"),
            &host.data.join("gone"),
        ),
        link(durable.join("state"), &home),
    ];
    let mut masks = placed_masks(&durable, &runtime, ORG_A, SAT_1);
    masks.folders.push(MaskedFolder {
        path: links[2].clone(),
        pre_create: true,
    });
    let mut made: Vec<PathPattern> = Vec::new();
    masks.push_denies(&mut made);
    let mut sandbox = SandboxSpec {
        enabled: true,
        writable_roots: Vec::new(),
        deny_write: Vec::new(),
        deny_read: made.clone(),
        allowed_unix_sockets: Vec::new(),
    };
    let mut policy = PermissionPolicy {
        version: 1,
        allow: Vec::new(),
        deny: Vec::new(),
    };

    assert!(!prune_nonexistent_bwrap_denies(&mut sandbox, &mut policy));
    let kept: Vec<String> = sandbox
        .deny_read
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    // Neighbours on disk are masked at their boundary, never traversed.
    for neighbour in [
        host.data.join(ORG_B),
        host.data.join(ORG_A).join("offices").join(SAT_2),
    ] {
        let raw = neighbour.display().to_string();
        assert!(kept.contains(&raw), "{raw} missing: {kept:?}");
        assert!(!kept.contains(&format!("{raw}/**")), "{kept:?}");
    }
    // A link stays the link itself, dangling or not, never what it points to.
    for link in &links {
        assert_masked_as_link(&kept, link);
    }
    // Absent: `bwrap` can't mask nothing, so the HQ's roots are dropped here and kept in the
    // unpruned list the approval bridge checks.
    for root in ["durable", "runtime"] {
        let hq_root = host.data.join(ORG_A).join(root).display().to_string();
        assert!(!kept.contains(&hq_root), "{kept:?}");
        assert!(made.iter().any(|p| p.as_str() == hq_root), "{made:?}");
    }
    for path in [&durable, &runtime, &home, &worktree] {
        assert_reachable(&kept, path);
    }
}
