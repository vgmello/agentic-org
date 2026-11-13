//! Executable build-input routes: closed target kinds and lint-only compiler flags.
use super::opener_inventory::{fixture, tracked};
use super::opener_policy::{cargo_config, config};

#[test]
fn sqlcipher_build_flags_admit_only_complete_lint_options() {
    for flag in [
        "-A",
        "-W",
        "-D",
        "-F",
        "--allow",
        "--warn",
        "--deny",
        "--forbid",
        "--force-warn",
    ] {
        for args in [
            vec![flag.to_string(), "dead_code".to_string()],
            vec![format!("{flag}=dead_code")],
        ] {
            let source = format!(
                "[build]\nrustflags={}",
                serde_json::to_string(&args).unwrap()
            );
            cargo_config(&source);
        }
        if flag.len() == 2 {
            cargo_config(&format!("build.rustflags=['{flag}dead_code']"));
        }
        for args in [
            vec![flag],
            vec![flag, ""],
            vec![flag, "-Clinker=tools/wrap"],
        ] {
            let source = format!("build.rustflags={}", serde_json::to_string(&args).unwrap());
            assert!(
                std::panic::catch_unwind(|| cargo_config(&source)).is_err(),
                "{source}"
            );
        }
    }
    for source in [
        "build.rustflags=[]",
        "build.rustflags=''",
        "build.rustflags=['-Dclippy::all','-Fwarnings']",
        "build.rustflags='-D clippy::all --forbid warnings'",
        "build.rustflags=['--force-warn=dead_code']",
    ] {
        cargo_config(source);
    }
    for flag in ["-A", "-W", "--allow", "--warn", "--force-warn"] {
        for lint in [
            "clippy::disallowed_methods",
            "clippy::disallowed-methods",
            "clippy::all",
            "warnings",
            "dead_code,clippy::disallowed_methods",
            "clippy::disallowed_methods,dead_code",
        ] {
            for args in [
                vec![flag.to_string(), lint.to_string()],
                vec![format!("{flag}={lint}")],
            ] {
                let source = format!("build.rustflags={}", serde_json::to_string(&args).unwrap());
                assert!(
                    std::panic::catch_unwind(|| cargo_config(&source)).is_err(),
                    "{source}"
                );
            }
            if flag.len() == 2 {
                assert!(std::panic::catch_unwind(|| cargo_config(&format!(
                    "build.rustflags=['{flag}{lint}']"
                )))
                .is_err());
            }
        }
    }
}

#[test]
fn sqlcipher_build_flags_refuse_compiler_execution_routes_in_every_scope() {
    for args in [
        vec!["-C", "linker=tools/wrap"],
        vec!["-Clinker=tools/wrap"],
        vec!["--codegen", "linker=tools/wrap"],
        vec!["--codegen=linker=tools/wrap"],
        vec!["-C", "linker-flavor=gcc"],
        vec!["-Clinker-flavor=gcc"],
        vec!["--codegen=linker-flavor=gcc"],
        vec!["-Clink-arg=-fuse-ld=tools/wrap"],
        vec!["-C", "link-args=-fuse-ld=tools/wrap"],
        vec!["--codegen=link-args=-fuse-ld=tools/wrap"],
        vec!["-Z", "unstable-options"],
        vec!["-Zunstable-options"],
        vec!["--extern", "helper=tools/libhelper.rlib"],
        vec!["--extern=helper=tools/libhelper.rlib"],
        vec!["-L", "tools"],
        vec!["-Ltools"],
        vec!["--library-path=tools"],
        vec!["--cfg", "unreviewed"],
        vec!["--cfg=unreviewed"],
        vec!["-l", "helper"],
        vec!["-lhelper"],
        vec!["--link=helper"],
        vec!["@tools/args"],
        vec!["--cap-lints", "allow"],
        vec!["--cap-lints=allow"],
        vec!["--unknown"],
        vec!["unreviewed"],
        vec!["--allowance=dead_code"],
        vec!["--deny-unrecognized=dead_code"],
        vec!["--forbidder=dead_code"],
        vec!["--warning=dead_code"],
        vec!["--force-warner=dead_code"],
        vec!["--allow="],
        vec!["-A="],
    ] {
        for scope in [
            "[build]\nrustflags",
            "[target.'cfg(unix)']\nrustflags",
            "target.x86_64-unknown-linux-gnu.rustflags",
            "\"build\".\"rustflags\"",
        ] {
            for ordered in [
                args.clone(),
                [vec!["-D", "dead_code"], args.clone()].concat(),
                [args.clone(), vec!["-F", "warnings"]].concat(),
            ] {
                let value = serde_json::to_string(&ordered).unwrap();
                let source = format!("{scope}={value}");
                assert!(
                    std::panic::catch_unwind(|| cargo_config(&source)).is_err(),
                    "{source}"
                );
                assert!(
                    std::panic::catch_unwind(|| config(&source)).is_err(),
                    "legacy: {source}"
                );
            }
        }
    }
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
    ] {
        for value in [
            "'-Clinker=tools/wrap'",
            "{value='--extern=helper=tools/helper.rlib',force=true}",
        ] {
            assert!(
                std::panic::catch_unwind(|| config(&format!("[env]\n{name}={value}"))).is_err()
            );
        }
    }
}

#[test]
fn sqlcipher_build_routes_refuse_tool_config_and_environment_at_inventory_entry() {
    let mut routes = vec![
        "[target.x86_64-unknown-linux-gnu]\nlinker='tools/wrap'".to_string(),
        "target.x86_64-unknown-linux-gnu.linker='tools/wrap'".to_string(),
        "[target.'cfg(unix)']\nlinker='tools/wrap'".to_string(),
        "[target.x86_64-unknown-linux-gnu]\nrunner='tools/run'".to_string(),
        "target.x86_64-unknown-linux-gnu.runner=['tools/run','arg']".to_string(),
        "[alias]\ncheck='!tools/check'".to_string(),
        "alias.check=['clippy','--config','tools/config']".to_string(),
        "include=['tools/config']".to_string(),
        "[unstable]\nbuild-std=['std']".to_string(),
        "[build]\nrustflags=['-Clinker=tools/wrap']".to_string(),
        "target.x86_64-unknown-linux-gnu.rustflags=['--extern=helper=tools/helper.rlib']"
            .to_string(),
    ];
    for name in [
        "rustc",
        "rustc-wrapper",
        "rustc-workspace-wrapper",
        "rustdoc",
        "rustdocflags",
    ] {
        routes.push(format!("build.{name}='tools/wrap'"));
    }
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER",
        "RUSTC_LINKER",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTDOC",
        "RUSTDOCFLAGS",
        "CLIPPY_ARGS",
        "CLIPPY_CONF_DIR",
    ] {
        routes.push(format!("[env]\n{name}='tools/wrap'"));
        routes.push(format!(
            "env.{name}={{value='tools/wrap',force=true,relative=true}}"
        ));
        routes.push(format!("[env.{name}]\nvalue='tools/wrap'"));
    }
    let paths = [
        ".cargo/config",
        ".cargo/config.toml",
        "nested/.cargo/config",
        "nested/.cargo/config.toml",
    ];
    let dir = fixture(&[
        ("src/lib.rs", "pub struct Allowed;"),
        (paths[0], ""),
        (paths[1], ""),
        (paths[2], ""),
        (paths[3], ""),
    ]);
    super::check_tracked_sources(&tracked(dir.path()));
    for source in routes {
        for path in paths {
            std::fs::write(dir.path().join(path), &source).unwrap();
            assert!(
                std::panic::catch_unwind(|| super::check_tracked_sources(&tracked(dir.path())))
                    .is_err(),
                "{path}: {source}"
            );
            std::fs::write(dir.path().join(path), "").unwrap();
        }
    }
}
