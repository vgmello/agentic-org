use super::opener_inventory::{fixture, tracked};
use super::opener_modules;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[test]
fn sqlcipher_modules_resolve_ordinary_inline_explicit_and_conditional_contexts() {
    for (source, files) in [
        (
            "mod custom;",
            vec![
                ("src/custom.rs", "#[path=\"child.rs\"] mod child;"),
                ("src/child.rs", ""),
            ],
        ),
        (
            "#[cfg_attr(test,path=\"custom.rs\")] mod chosen;",
            vec![("src/custom.rs", ""), ("src/chosen.rs", "")],
        ),
        (
            "#[doc=\"ordinary\"] #[path=\"custom.rs\"] mod chosen;",
            vec![("src/custom.rs", "mod child;"), ("src/child.rs", "")],
        ),
        (
            "#[clippy::anything] mod custom;",
            vec![("src/custom.rs", "mod child;"), ("src/custom/child.rs", "")],
        ),
        (
            "#[cfg_attr(path,path=\"custom.rs\")] mod chosen;",
            vec![
                ("src/chosen.rs", ""),
                ("src/custom.rs", "mod child;"),
                ("src/child.rs", ""),
            ],
        ),
        (
            "#[path=\"../build/custom.rs\"] mod chosen;",
            vec![("build/custom.rs", "mod child;"), ("build/child.rs", "")],
        ),
        (
            "mod custom;",
            vec![("src/custom.rs", "mod child;"), ("src/custom/child.rs", "")],
        ),
        (
            "mod custom;",
            vec![
                ("src/custom/mod.rs", "mod child;"),
                ("src/custom/child.rs", ""),
            ],
        ),
        (
            "#[path=\"custom.rs\"] mod custom;",
            vec![("src/custom.rs", "mod child;"), ("src/child.rs", "")],
        ),
        (
            "mod inline { mod custom; }",
            vec![
                ("src/inline/custom.rs", "mod child;"),
                ("src/inline/custom/child.rs", ""),
            ],
        ),
        ("mod r#type { mod child; }", vec![("src/type/child.rs", "")]),
        (
            "#[cfg_attr(test, cfg_attr(unix, path=\"custom.rs\"))] mod chosen;",
            vec![
                ("src/custom.rs", "mod child;"),
                ("src/chosen.rs", ""),
                ("src/child.rs", ""),
            ],
        ),
        (
            "#[cfg_attr(test, path=\"first.rs\", path=\"second.rs\")] mod chosen;",
            vec![
                ("src/chosen.rs", ""),
                ("src/first.rs", ""),
                ("src/second.rs", ""),
            ],
        ),
        (
            "#[r#path=r#\"custom.rs\"#] mod chosen;",
            vec![("src/custom.rs", "mod child;"), ("src/child.rs", "")],
        ),
        (
            "#[path=\"\\u{63}ustom.rs\"] mod chosen;",
            vec![("src/custom.rs", "mod child;"), ("src/child.rs", "")],
        ),
        (
            "#[r#cfg_attr(test,r#path=\"custom.rs\")] mod chosen;",
            vec![
                ("src/chosen.rs", ""),
                ("src/custom.rs", "mod child;"),
                ("src/child.rs", ""),
            ],
        ),
    ] {
        let mut inputs = vec![("src/lib.rs", source)];
        inputs.extend(files.clone());
        let dir = fixture(&inputs);
        let scope = tracked(dir.path());
        super::check_tracked_sources(&scope);
        opener_modules::check(&scope, [scope.root.join("src/lib.rs")]);
        let child = files.last().unwrap().0;
        super::bounded_output(
            std::process::Command::new("git")
                .current_dir(dir.path())
                .args(["rm", "--cached", child]),
            "untrack module child",
        );
        let scope = tracked(dir.path());
        assert!(
            std::panic::catch_unwind(|| opener_modules::check(
                &scope,
                [scope.root.join("src/lib.rs")]
            ))
            .is_err(),
            "accepted untracked child for {source}"
        );
    }
}

#[test]
fn sqlcipher_modules_visit_both_contexts_of_a_shared_cargo_root() {
    let dir = fixture(&[
        ("Cargo.toml", "[workspace]\nmembers=['a','b']\nresolver='2'"),
        ("Cargo.lock", "version=4\n[[package]]\nname='a'\nversion='0.1.0'\n[[package]]\nname='b'\nversion='0.1.0'"),
        ("a/Cargo.toml", "[package]\nname='a'\nversion='0.1.0'\nedition='2021'"),
        ("a/src/lib.rs", "mod shared;"), ("a/src/shared.rs", "mod child;"),
        ("a/src/child.rs", "pub struct RootChild;"),
        ("b/Cargo.toml", "[package]\nname='b'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='../a/src/shared.rs'"),
    ]);
    std::fs::create_dir_all(dir.path().join("a/src/shared")).unwrap();
    std::fs::write(
        dir.path().join("a/src/shared/child.rs"),
        "#![allow(warnings)]",
    )
    .unwrap();
    let scope = tracked(dir.path());
    let roots = super::opener_sources::cargo_roots(&scope);
    assert_eq!(
        roots,
        BTreeSet::from([
            scope.root.join("a/src/lib.rs"),
            scope.root.join("a/src/shared.rs")
        ])
    );
    for reverse in [false, true] {
        let mut order = roots.iter().cloned().collect::<Vec<_>>();
        if reverse {
            order.reverse();
        }
        assert!(std::panic::catch_unwind(|| opener_modules::check(&scope, order)).is_err());
    }
    std::fs::write(
        dir.path().join("a/src/shared/child.rs"),
        "pub struct ModuleChild;",
    )
    .unwrap();
    super::bounded_output(
        std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["add", "--force", "a/src/shared/child.rs"]),
        "track module child",
    );
    let scope = tracked(dir.path());
    for reverse in [false, true] {
        let mut order = roots.iter().cloned().collect::<Vec<_>>();
        if reverse {
            order.reverse();
        }
        opener_modules::check(&scope, order);
    }
}

#[test]
fn sqlcipher_modules_restore_parent_and_sibling_inline_directories() {
    let dir = fixture(&[
        (
            "src/lib.rs",
            "mod first { mod nested { mod leaf; } mod own; } mod sibling { mod leaf; } mod root;",
        ),
        ("src/first/nested/leaf.rs", ""),
        ("src/first/own.rs", ""),
        ("src/sibling/leaf.rs", ""),
        ("src/root.rs", ""),
    ]);
    let scope = tracked(dir.path());
    opener_modules::check(&scope, [scope.root.join("src/lib.rs")]);
}

#[test]
fn sqlcipher_modules_refuse_missing_ambiguous_and_invalid_inputs() {
    for (source, files) in [
        ("mod child;", vec![]),
        (
            "mod child;",
            vec![("src/child.rs", ""), ("src/child/mod.rs", "")],
        ),
        ("#[path=\"/tmp/outside.rs\"] mod child;", vec![]),
        (
            "#[path=\"child.inc\"] mod child;",
            vec![("src/child.inc", "")],
        ),
        (
            "#[path=concat!(\"child\",\".rs\")] mod child;",
            vec![("src/child.rs", "")],
        ),
        ("#[path] mod child;", vec![("src/child.rs", "")]),
        ("#[path=7] mod child;", vec![("src/child.rs", "")]),
        (
            "#[cfg_attr(test,path=)] mod child;",
            vec![("src/child.rs", "")],
        ),
        ("#[cfg_attr] mod child;", vec![("src/child.rs", "")]),
        (
            "#[cfg_attr(test,path=\"child.rs\"] mod child;",
            vec![("src/child.rs", "")],
        ),
        ("mod child;", vec![("src/child.rs", "{")]),
    ] {
        let mut inputs = vec![("src/lib.rs", source)];
        inputs.extend(files);
        let dir = fixture(&inputs);
        let scope = tracked(dir.path());
        assert!(
            std::panic::catch_unwind(|| opener_modules::check(
                &scope,
                [scope.root.join("src/lib.rs")]
            ))
            .is_err(),
            "accepted {source}"
        );
    }
    let source = format!("mod {};", "a".repeat(300));
    let dir = fixture(&[("src/lib.rs", &source)]);
    let scope = tracked(dir.path());
    assert!(std::panic::catch_unwind(|| opener_modules::check(
        &scope,
        [scope.root.join("src/lib.rs")]
    ))
    .is_err());
}

#[test]
fn sqlcipher_modules_require_source_scope_at_the_read_site() {
    use super::opener_scope::SourceKind;
    let dir = fixture(&[("src/lib.rs", ""), ("src/child.rs", ""), ("other.rs", "")]);
    let mut scope = tracked(dir.path());
    let root = scope.root.join("src/lib.rs");
    opener_modules::check(&scope, [root.clone()]);
    scope.files.remove(&root);
    assert!(std::panic::catch_unwind(|| opener_modules::check(&scope, [root.clone()])).is_err());
    scope.files.insert(root.clone(), SourceKind::Config);
    assert!(std::panic::catch_unwind(|| opener_modules::check(&scope, [root.clone()])).is_err());
    scope.files.insert(root.clone(), SourceKind::Rust);
    std::fs::remove_file(&root).unwrap();
    std::os::unix::fs::symlink(scope.root.join("other.rs"), &root).unwrap();
    assert!(std::panic::catch_unwind(|| opener_modules::check(&scope, [root])).is_err());
    assert!(std::panic::catch_unwind(|| opener_modules::check(
        &scope,
        [PathBuf::from("/tmp/outside.rs")]
    ))
    .is_err());
}

#[test]
fn sqlcipher_module_paths_refuse_absolute_paths_even_inside_scope() {
    let dir = fixture(&[("src/lib.rs", ""), ("src/child.rs", "")]);
    let scope = tracked(dir.path());
    let source = format!(
        "#[path={:?}] mod child;",
        scope.root.join("src/child.rs").to_str().unwrap()
    );
    std::fs::write(scope.root.join("src/lib.rs"), source).unwrap();
    assert!(std::panic::catch_unwind(|| opener_modules::check(
        &scope,
        [scope.root.join("src/lib.rs")]
    ))
    .is_err());
}

#[test]
fn sqlcipher_modules_deduplicate_cyclic_explicit_contexts() {
    let dir = fixture(&[("src/lib.rs", "#[path=\"lib.rs\"] mod again;")]);
    let scope = tracked(dir.path());
    opener_modules::check(&scope, [scope.root.join("src/lib.rs")]);
}

#[test]
fn sqlcipher_inline_module_directory_matches_root_and_file_contexts() {
    for (root, source, modules, child) in [
        (
            "src/lib.rs",
            "mod foo { mod bar; }",
            vec![],
            "src/foo/bar.rs",
        ),
        (
            "src/main.rs",
            "mod foo { mod bar; }",
            vec![],
            "src/foo/bar.rs",
        ),
        (
            "src/mod.rs",
            "mod foo { mod bar; }",
            vec![],
            "src/foo/bar.rs",
        ),
        (
            "src/custom.rs",
            "mod foo { mod bar; }",
            vec![],
            "src/foo/bar.rs",
        ),
        (
            "src/lib.rs",
            "mod a;",
            vec![("src/a.rs", "mod foo { mod bar; }")],
            "src/a/foo/bar.rs",
        ),
        (
            "src/lib.rs",
            "mod a;",
            vec![("src/a/mod.rs", "mod foo { mod bar; }")],
            "src/a/foo/bar.rs",
        ),
        (
            "src/lib.rs",
            "#[path=\"a.rs\"] mod a;",
            vec![("src/a.rs", "mod foo { mod bar; }")],
            "src/foo/bar.rs",
        ),
    ] {
        let mut files = vec![(root, source), (child, "")];
        files.extend(modules);
        let dir = fixture(&files);
        let scope = tracked(dir.path());
        super::check_tracked_sources(&scope);
        opener_modules::check(&scope, [scope.root.join(root)]);
    }
}

#[test]
fn sqlcipher_module_lookup_refuses_unknown_candidate_errors() {
    // The directory component fits; appending .rs exceeds NAME_MAX. A valid mod.rs
    // sibling must not hide an I/O error inspecting the other candidate.
    let name = "a".repeat(253);
    let child = format!("src/{name}/mod.rs");
    let source = format!("mod {name};");
    let dir = fixture(&[("src/lib.rs", &source), (&child, "")]);
    let scope = tracked(dir.path());
    assert!(std::panic::catch_unwind(|| opener_modules::check(
        &scope,
        [scope.root.join("src/lib.rs")]
    ))
    .is_err());
}

#[test]
fn sqlcipher_source_closure_requires_tracking_in_formerly_skipped_directories() {
    for directory in [
        "build",
        "dist",
        "target",
        "node_modules",
        ".git",
        ".svelte-kit",
    ] {
        let source = format!("#[path=\"../{directory}/custom.rs\"] mod custom;");
        let file = format!("{directory}/custom.rs");
        // Git's own directory is never a source inventory entry.
        if directory == ".git" {
            let dir = fixture(&[("src/lib.rs", &source)]);
            std::fs::create_dir_all(dir.path().join(".git")).unwrap();
            std::fs::write(dir.path().join(&file), "").unwrap();
            assert!(std::panic::catch_unwind(|| opener_modules::check(
                &tracked(dir.path()),
                [dir.path().canonicalize().unwrap().join("src/lib.rs")]
            ))
            .is_err());
        } else {
            let dir = fixture(&[("src/lib.rs", &source), (&file, "")]);
            let scope = tracked(dir.path());
            super::check_tracked_sources(&scope);
            opener_modules::check(&scope, [scope.root.join("src/lib.rs")]);
            super::bounded_output(
                std::process::Command::new("git")
                    .current_dir(dir.path())
                    .args(["rm", "--cached", &file]),
                "untrack formerly skipped directory module",
            );
            let scope = tracked(dir.path());
            assert!(std::panic::catch_unwind(|| opener_modules::check(
                &scope,
                [scope.root.join("src/lib.rs")]
            ))
            .is_err());
        }
    }
}
