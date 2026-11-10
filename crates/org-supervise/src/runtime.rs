//! The single mapping from the runtime port to generic spawn primitives.
//! Lifecycle instructions are retained explicitly and mapped into generic core types;
//! preparing a launch does not execute them. This is not a complete session runner.

use std::io;
use std::path::Path;
use std::time::Duration;

use org_runtime::{
    CleanupPath, EnvName, FileLocation, LaunchContext, LaunchPlan, ProbeContext, ProbePlan,
    SessionId, Stdio, StopPlan,
};

use crate::{files, unsupported, Environment, PreparedSpawn, SessionMark, SpawnSpec};

/// Translate the runtime port's exhaustive stop instructions into the generic policy.
pub fn stop_policy(plan: &StopPlan) -> io::Result<crate::StopPolicy> {
    let steps = plan
        .steps
        .iter()
        .map(|step| match *step {
            org_runtime::StopStep::CloseStdin { grace_ms } => crate::StopStep::CloseStdin {
                grace: Duration::from_millis(grace_ms),
            },
            org_runtime::StopStep::Signal {
                signal,
                target,
                grace_ms,
            } => crate::StopStep::Signal {
                signal: match signal {
                    org_runtime::StopSignal::Interrupt => crate::StopSignal::Interrupt,
                    org_runtime::StopSignal::Terminate => crate::StopSignal::Terminate,
                },
                target: match target {
                    org_runtime::SignalTarget::Leader => crate::SignalTarget::Leader,
                    org_runtime::SignalTarget::Group => crate::SignalTarget::Group,
                },
                grace: Duration::from_millis(grace_ms),
            },
        })
        .collect();
    crate::StopPolicy::new(steps)
}

/// Retain validated runtime socket templates while keeping runtime types out of the core.
pub fn socket_cleanup(paths: &[CleanupPath]) -> io::Result<Vec<crate::SocketCleanup>> {
    paths
        .iter()
        .map(|path| {
            let relative = path
                .template()
                .strip_prefix(path.root().as_str())
                .and_then(|p| p.strip_prefix('/'))
                .ok_or_else(|| crate::invalid("socket template outside its allowed root"))?;
            crate::SocketCleanup::new(path.root().as_str().into(), relative.to_owned())
        })
        .collect()
}

/// Whether the caller closes stdin after initial lines or drives a ProbeSession itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeMode {
    /// Close stdin after the initial lines, collect bounded output and parse it.
    OneShot,
    /// Leave stdin open for the caller's sans-I/O ProbeSession.
    Conversation,
}

/// A probe's process and instructions. The caller retains all pipe and protocol I/O.
pub struct RuntimeProbe {
    /// Record mark, spawn, then record identity with the same callbacks as sessions.
    pub prepared: PreparedSpawn,
    /// Initial stdin lines, in order.
    pub stdin_lines: Vec<org_runtime::Line>,
    /// Stdin/protocol shape.
    pub mode: ProbeMode,
    /// Overall deadline duration, fixed by the caller when the process starts.
    pub timeout: Duration,
    /// Fresh empty cwd under the supplied private session temp tree. Owned by the
    /// prepared process and removed on failed preparation/spawn or cleanup_and_reap.
    pub cwd: std::path::PathBuf,
}

/// Prepare a probe without borrowing agent authority or writing a workspace. `temp_root`
/// must be an existing, real-path own-UID/0700 session temp folder allocated by the caller.
/// Refuse all context-only names (ORG_*, GIT_CONFIG_*, GH_CONFIG_DIR) rather than
/// silently stripping accidentally supplied agent or tool authority.
/// The fresh private cwd protects shared folders from CLIs that write relative dotfiles.
/// No ProbeSession is called here, and neither stdout nor stderr is consumed.
pub fn prepare_probe(
    plan: ProbePlan,
    ctx: &ProbeContext,
    environment: Environment,
    mark: SessionMark,
    temp_root: &Path,
    mode: ProbeMode,
) -> io::Result<RuntimeProbe> {
    if environment.0.keys().any(|name| {
        let name = name.as_encoded_bytes();
        name.starts_with(b"ORG_") || name.starts_with(b"GIT_CONFIG_") || name == b"GH_CONFIG_DIR"
    }) {
        return Err(crate::invalid(
            "probe environment must not carry context-only variables",
        ));
    }
    if plan.program != ctx.runtime_path {
        return Err(crate::invalid(
            "probe executable differs from the recorded runtime path",
        ));
    }
    let additions = plan
        .env_add
        .into_iter()
        .map(|(name, value)| (name.as_str().into(), value.into()))
        .collect::<Vec<_>>();
    let environment = environment.with_additions(&additions, REVIEWED_ENV_SWITCHES)?;
    crate::platform::validate()?;
    let directory = files::PrivateDirectory::new(temp_root)?;
    let cwd = directory.path().to_owned();
    let prepared = PreparedSpawn::new(
        SpawnSpec {
            program: plan.program,
            args: plan.args.into_iter().map(Into::into).collect(),
            cwd: cwd.clone(),
            environment,
        },
        mark,
    )?
    .with_cleanup_directory(directory);
    Ok(RuntimeProbe {
        prepared,
        stdin_lines: plan.stdin_lines,
        mode,
        timeout: Duration::from_millis(plan.timeout_ms),
        cwd,
    })
}

// Runtime-policy names belong at this mapping boundary, never in generic Environment.
const REVIEWED_ENV_SWITCHES: &[&str] = &[
    "DISABLE_AUTOUPDATER",
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS",
];

/// The runtime-only lifecycle metadata, retained for the pump/stop layer.
pub struct Lifecycle {
    /// Graceful-stop instructions, not executed by preparation or minimal ownership Drop.
    pub stop: StopPlan,
    /// Socket templates, mapped with socket_cleanup and cleaned before reaping.
    pub cleanup_after_exit: Vec<CleanupPath>,
    /// Secret variable names the pump must redact from protocol and stderr output.
    pub secret_env_names: Vec<EnvName>,
    /// Conversation identity if known at launch.
    pub runtime_session_id: Option<SessionId>,
}

/// Prepared generic spawn plus lifecycle data. Readers and protocol remain caller-owned.
pub struct RuntimeLaunch {
    /// Generic process preparation. Record the mark, then spawn and record identity.
    pub prepared: PreparedSpawn,
    /// Instructions for the lifecycle layer; preparation does not perform a graceful stop.
    pub lifecycle: Lifecycle,
}

/// Validate a piped launch, install planned files, and retain every lifecycle field.
/// `git_exclude` needs the caller's repository capability (X-09), so it fails closed here.
/// PTYs are unavailable until slice 4. Neither rejection writes any planned file.
pub fn prepare(
    plan: LaunchPlan,
    ctx: &LaunchContext,
    environment: Environment,
    mark: SessionMark,
) -> io::Result<RuntimeLaunch> {
    let LaunchPlan {
        program,
        args,
        cwd,
        env_add,
        dirs,
        files: planned_files,
        stdio,
        runtime_session_id,
        stop,
        cleanup_after_exit,
        secret_env_names,
    } = plan;
    if stdio != Stdio::Piped {
        return Err(unsupported("PTY spawning is not implemented in this slice"));
    }
    if planned_files.iter().any(|file| file.git_exclude) {
        return Err(unsupported(
            "git exclusion needs a caller-owned repository capability",
        ));
    }
    if program != ctx.runtime_path {
        return Err(crate::invalid(
            "plan executable differs from the recorded runtime path",
        ));
    }
    let additions = env_add
        .into_iter()
        .map(|(name, value)| (name.as_str().into(), value.into()))
        .collect::<Vec<_>>();
    let environment = environment.with_additions(&additions, REVIEWED_ENV_SWITCHES)?;
    let spec = SpawnSpec {
        program,
        args: args.into_iter().map(Into::into).collect(),
        cwd: cwd.as_str().into(),
        environment,
    };
    let prepared = PreparedSpawn::new(spec, mark)?;
    let location = |kind| match kind {
        FileLocation::RuntimeDir => Path::new(ctx.runtime_dir.as_str()),
        FileLocation::Workspace => Path::new(cwd.as_str()),
    };
    for dir in dirs {
        files::create_dir(location(dir.location), Path::new(dir.rel_path.as_str()))?;
    }
    for file in planned_files {
        files::write_file(
            location(file.location),
            Path::new(file.rel_path.as_str()),
            &file.contents,
        )?;
    }
    Ok(RuntimeLaunch {
        prepared,
        lifecycle: Lifecycle {
            stop,
            cleanup_after_exit,
            secret_env_names,
            runtime_session_id,
        },
    })
}
