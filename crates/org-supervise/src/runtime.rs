//! The single mapping from the runtime port to generic spawn primitives.
//! Lifecycle instructions are returned explicitly for the later stop/cleanup layer;
//! preparing a launch does not execute them. This is not a complete session runner.

use std::io;
use std::path::Path;

use org_runtime::{
    CleanupPath, EnvName, FileLocation, LaunchContext, LaunchPlan, SessionId, Stdio, StopPlan,
};

use crate::{files, unsupported, Environment, PreparedSpawn, SessionMark, SpawnSpec};

/// The runtime-only lifecycle metadata, retained for the pump/stop layer.
pub struct Lifecycle {
    /// Graceful-stop instructions, not executed by preparation or minimal ownership Drop.
    pub stop: StopPlan,
    /// Socket templates, to be cleaned before reaping by the later lifecycle layer.
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
    let additions = env_add
        .into_iter()
        .map(|(name, value)| (name.as_str().into(), value.into()))
        .collect::<Vec<_>>();
    let environment = environment.with_runtime_additions(&additions)?;
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
