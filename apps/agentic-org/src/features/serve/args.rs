//! `serve`'s command line and the host configuration it builds. A release build trusts only
//! what it was built with and refuses the arguments that would change it.

use std::path::PathBuf;

use org_host::Config;

pub(super) const USAGE: &str =
    "usage: agentic-org serve [--version] | --name <instance> | --setup <file> \
                     [--name <instance>] | (debug builds only) [--home <absolute path>] \
                     [--server-bin <path>] [--app-team-id <team id>]";

/// Which kind of build this is: a release build refuses the arguments that change what it
/// trusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Build {
    Debug,
    Release,
}

impl Build {
    pub(super) const CURRENT: Build = if cfg!(debug_assertions) {
        Build::Debug
    } else {
        Build::Release
    };
}

/// What the command line asks for.
#[derive(Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    not(debug_assertions),
    allow(
        dead_code,
        reason = "release builds read only `version` and refuse dev CLI options"
    )
)]
pub(super) struct Args {
    pub(super) version: bool,
    /// The registered instance to serve.
    pub(super) name: Option<String>,
    /// The foreground mode: the setup configuration to apply first.
    pub(super) setup: Option<PathBuf>,
    pub(super) home: Option<PathBuf>,
    pub(super) server_bin: Option<PathBuf>,
    pub(super) app_team_id: Option<String>,
}

/// rule at all (`AppPolicy::with_dev_app` and `Config::dev_app_path` exist only in debug
/// builds, so release code never calls this).
#[cfg_attr(
    not(debug_assertions),
    allow(
        dead_code,
        reason = "release builds have no development CLI peer policy; unit tests cover it"
    )
)]
pub(super) fn dev_cli_peer(
    build: Build,
    named: bool,
    built_in: Option<PathBuf>,
    current_exe: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match (build, named, built_in) {
        (Build::Release, _, built_in) => built_in,
        (Build::Debug, true, None) => current_exe(),
        (Build::Debug, _, built_in) => built_in,
    }
}

/// A release build's config: only what it was built with, bound to the verified installed
/// release in the account's protected release store before any custody or socket is opened.
#[cfg(not(debug_assertions))]
pub(super) fn config(_args: Args) -> Result<Config, String> {
    let config = Config::trusted()?;
    let store_dir = org_host::default_release_store_dir()?;
    let store = org_host::release::ReleaseStore::open(&store_dir).map_err(|e| {
        format!(
            "opening release store ({}): {e}; install a signed release first with `agentic-org release install <bundle.tar.gz>`",
            store_dir.display()
        )
    })?;
    let release = store.admit_self().map_err(|e| {
        format!(
            "admitting installed release ({}): {e}; run the installed binary from the release store or reinstall with `agentic-org release install <bundle.tar.gz>`",
            store_dir.display()
        )
    })?;
    config
        .with_runtime_release(release)
        .map_err(|e| e.to_string())
}

/// A debug build's config: what it was built with, with the arguments' overrides and the
/// development settings.
#[cfg(debug_assertions)]
pub(super) fn config(args: Args) -> Result<Config, String> {
    let home = match args.home {
        Some(home) => home,
        None => org_host::default_office_home()
            .ok_or("no office home: neither ORG_OFFICE_HOME nor HOME is an absolute path")?,
    };
    let server_bin = match args.server_bin {
        // Servers run with `/` as their working directory, so a relative path must be
        // resolved here, against ours.
        Some(path) => {
            std::path::absolute(&path).map_err(|e| format!("resolving {}: {e}", path.display()))?
        }
        None => org_host::default_server_program()
            .map_err(|e| format!("finding the office server: {e}"))?,
    };
    let mut config = Config::new(home, server_bin);
    config.set_app_team_id(
        args.app_team_id
            .or(org_host::APP_TEAM_ID.map(str::to_owned)),
    );
    // The development app, fixed when this binary was compiled.
    config.dev_app_path = option_env!("ORG_DEV_APP_PATH").map(PathBuf::from);
    // The development key folder, resolved against our working directory.
    config.dev_key_dir = match std::env::var_os("ORG_OFFICE_DEV_KEY_DIR") {
        Some(dir) if !dir.is_empty() => Some(
            std::path::absolute(&dir)
                .map_err(|e| format!("resolving ORG_OFFICE_DEV_KEY_DIR: {e}"))?,
        ),
        _ => None,
    };
    Ok(config)
}

/// Parses the arguments (without the program name) for a `build`. Pure: it reads nothing else
/// and exits nothing. A release build refuses `--home`, `--server-bin` and `--app-team-id`.
pub(super) fn parse_args(
    args: impl IntoIterator<Item = String>,
    build: Build,
) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--version" {
            parsed.version = true;
            continue;
        }
        if arg == "--name" {
            let value = args
                .next()
                .ok_or_else(|| format!("--name needs a value\n{USAGE}"))?;
            if parsed.name.replace(value).is_some() {
                return Err(format!("--name may be given once\n{USAGE}"));
            }
            continue;
        }
        if arg == "--setup" {
            let value = args
                .next()
                .ok_or_else(|| format!("--setup needs a file\n{USAGE}"))?;
            if parsed.setup.replace(PathBuf::from(value)).is_some() {
                return Err(format!("--setup may be given once\n{USAGE}"));
            }
            continue;
        }
        let known = ["--home", "--server-bin", "--app-team-id"];
        if !known.contains(&arg.as_str()) {
            return Err(format!("unexpected argument {arg:?}\n{USAGE}"));
        }
        if build == Build::Release {
            return Err(format!(
                "{arg} isn't accepted by a release build: it uses only the office home, office \
                 server and app team id it was built with"
            ));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?;
        match arg.as_str() {
            "--home" => parsed.home = Some(PathBuf::from(value)),
            "--server-bin" => parsed.server_bin = Some(PathBuf::from(value)),
            _ => parsed.app_team_id = Some(value),
        }
    }
    if (parsed.name.is_some() || parsed.setup.is_some()) && parsed.home.is_some() {
        return Err(format!(
            "--name and --setup can't be combined with --home: a registered instance has its \
             own home\n{USAGE}"
        ));
    }
    Ok(parsed)
}
