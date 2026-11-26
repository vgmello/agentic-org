//! Pure rules every adapter applies the same way while it plans a launch.
//!
//! They live in the port so that a refusal reads the same whichever runtime gives it, and so
//! that a fix to one rule reaches every adapter. None of them does I/O or knows a runtime:
//! what differs between runtimes (the concrete tools of a capability, the environment
//! switches, the name of the config-dir variable) comes in as arguments.

use crate::{
    EnvName, LaunchContext, PlanError, SessionKind, SessionSpec, SpecPath, ToolCapability, ToolName,
};

/// Refuses a spec or context that puts `/` where an adapter would grant or write: the
/// workspace, [`LaunchContext::tmpdir`], [`LaunchContext::runtime_dir`],
/// [`LaunchContext::config_dir`] or one of the sandbox's writable roots
/// ([`PlanError::InvalidSpec`], naming the field).
pub fn validate_plan_paths(spec: &SessionSpec, ctx: &LaunchContext) -> Result<(), PlanError> {
    if spec.workspace.is_root() {
        return Err(PlanError::InvalidSpec(
            "workspace must not be `/`".to_owned(),
        ));
    }
    if ctx.tmpdir.is_root() {
        return Err(PlanError::InvalidSpec("tmpdir must not be `/`".to_owned()));
    }
    if ctx.runtime_dir.is_root() {
        return Err(PlanError::InvalidSpec(
            "runtime_dir must not be `/`".to_owned(),
        ));
    }
    if ctx.config_dir.as_ref().is_some_and(SpecPath::is_root) {
        return Err(PlanError::InvalidSpec(
            "config_dir must not be `/`".to_owned(),
        ));
    }
    if spec.sandbox.writable_roots.iter().any(SpecPath::is_root) {
        return Err(PlanError::InvalidSpec(
            "sandbox writable_roots must not include `/`".to_owned(),
        ));
    }
    Ok(())
}

/// Accepts the session kinds every runtime runs today ([`SessionKind::Home`],
/// [`SessionKind::Task`], [`SessionKind::OneOff`]) and refuses any other with
/// [`PlanError::Unsupported`]`("session kind")`, so a kind added to the port later is refused
/// until an adapter decides how to run it.
pub fn validate_standard_session_kind(kind: &SessionKind) -> Result<(), PlanError> {
    match kind {
        SessionKind::Home {} | SessionKind::Task { .. } | SessionKind::OneOff { .. } => Ok(()),
        // Unreachable inside this crate today. The expectation fails once a kind is added, so
        // whoever adds it decides here whether every runtime runs it.
        #[expect(
            unreachable_patterns,
            reason = "`SessionKind` is non-exhaustive: a kind added later is refused here"
        )]
        _ => Err(PlanError::Unsupported("session kind")),
    }
}

/// Expands a [`SessionSpec::tools`] allowlist into the runtime's concrete tool names, in
/// order and without duplicates. A `cap:*` token becomes the tools `capability_tools` gives
/// for its [`ToolCapability`]; a concrete tool name passes through. Fails closed with
/// [`PlanError::Unsupported`]`("tool capability")` on a `cap:*` token that isn't a known
/// capability or that `capability_tools` has no tools for.
pub fn expand_capability_allowlist(
    tools: &[ToolName],
    capability_tools: impl Fn(ToolCapability) -> Option<&'static [&'static str]>,
) -> Result<Vec<String>, PlanError> {
    let mut expanded: Vec<String> = Vec::new();
    for tool in tools {
        let raw = tool.as_str();
        if ToolCapability::is_capability_token(raw) {
            let cap = ToolCapability::parse_rule_token(raw)
                .ok_or(PlanError::Unsupported("tool capability"))?;
            let concrete =
                capability_tools(cap).ok_or(PlanError::Unsupported("tool capability"))?;
            for &name in concrete {
                if !expanded.iter().any(|existing| existing == name) {
                    expanded.push(name.to_owned());
                }
            }
        } else if !expanded.iter().any(|existing| existing == raw) {
            expanded.push(raw.to_owned());
        }
    }
    Ok(expanded)
}

/// An adapter's fixed list of environment variable names as [`EnvName`]s, in order (for
/// [`crate::LaunchPlan::auth_env_names`] and [`crate::LaunchPlan::secret_env_names`]).
///
/// # Panics
/// If a name isn't a valid [`EnvName`]: the names are the adapter's own constants, so that is
/// a bug every test of the adapter's plan catches.
pub fn static_env_names(names: &[&'static str]) -> Vec<EnvName> {
    names
        .iter()
        .map(|&name| EnvName::try_from(name).expect("static env name is valid"))
        .collect()
}

/// An adapter's [`crate::LaunchPlan::env_add`]: its `fixed` switches in order, then
/// `config_dir_env` set to [`LaunchContext::config_dir`] when the office gave one.
///
/// # Panics
/// If a name isn't a valid [`EnvName`]: the names are the adapter's own constants, so that is
/// a bug every test of the adapter's plan catches.
pub fn runtime_env_add(
    fixed: &[(&'static str, &'static str)],
    config_dir_env: &'static str,
    ctx: &LaunchContext,
) -> Vec<(EnvName, String)> {
    let mut env: Vec<(EnvName, String)> = fixed
        .iter()
        .map(|&(name, value)| {
            (
                EnvName::try_from(name).expect("static env name is valid"),
                value.to_owned(),
            )
        })
        .collect();
    if let Some(cfg_dir) = &ctx.config_dir {
        env.push((
            EnvName::try_from(config_dir_env).expect("static env name is valid"),
            cfg_dir.as_str().to_owned(),
        ));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentIdentity, AgentName, PermissionMode, PermissionPolicy, PlainId, RuntimeId, SandboxSpec,
    };
    use std::path::PathBuf;

    fn p(path: &str) -> SpecPath {
        SpecPath::try_from(path).unwrap()
    }

    fn spec() -> SessionSpec {
        SessionSpec {
            runtime: RuntimeId::try_from("claude").unwrap(),
            kind: SessionKind::Home {},
            agent: AgentIdentity {
                name: AgentName::try_from("agent").unwrap(),
                description: "Agent".to_owned(),
                prompt: "You are agent.".to_owned(),
            },
            model: None,
            effort: None,
            workspace: p("/work/t-1"),
            read_only_dirs: Vec::new(),
            instructions: String::new(),
            repository_instructions: String::new(),
            permission_mode: PermissionMode::Ask,
            policy: PermissionPolicy {
                version: 1,
                allow: Vec::new(),
                deny: Vec::new(),
            },
            sandbox: SandboxSpec {
                enabled: true,
                writable_roots: vec![p("/work/t-1")],
                deny_write: Vec::new(),
                deny_read: Vec::new(),
                allowed_unix_sockets: Vec::new(),
            },
            mcp_servers: Default::default(),
            tools: None,
        }
    }

    fn ctx() -> LaunchContext {
        LaunchContext {
            runtime_path: PathBuf::from("/usr/local/bin/runtime"),
            runtime_version: None,
            runtime_dir: p("/office/agents/a1/runtime"),
            tmpdir: p("/tmp/session-1"),
            office_env_names: Vec::new(),
            new_session_id: None,
            resume: None,
            config_dir: Some(p("/office/agents/a1/config")),
        }
    }

    fn invalid(why: &str) -> Result<(), PlanError> {
        Err(PlanError::InvalidSpec(why.to_owned()))
    }

    #[test]
    fn plan_paths_accept_ordinary_paths_and_refuse_root_with_the_field_named() {
        assert_eq!(validate_plan_paths(&spec(), &ctx()), Ok(()));
        let without_config = LaunchContext {
            config_dir: None,
            ..ctx()
        };
        assert_eq!(validate_plan_paths(&spec(), &without_config), Ok(()));

        let mut root_workspace = spec();
        root_workspace.workspace = p("/");
        assert_eq!(
            validate_plan_paths(&root_workspace, &ctx()),
            invalid("workspace must not be `/`")
        );

        let mut root_ctx = ctx();
        root_ctx.tmpdir = p("/");
        assert_eq!(
            validate_plan_paths(&spec(), &root_ctx),
            invalid("tmpdir must not be `/`")
        );

        let mut root_ctx = ctx();
        root_ctx.runtime_dir = p("/");
        assert_eq!(
            validate_plan_paths(&spec(), &root_ctx),
            invalid("runtime_dir must not be `/`")
        );

        let mut root_ctx = ctx();
        root_ctx.config_dir = Some(p("/"));
        assert_eq!(
            validate_plan_paths(&spec(), &root_ctx),
            invalid("config_dir must not be `/`")
        );

        let mut root_writable = spec();
        root_writable.sandbox.writable_roots.push(p("/"));
        assert_eq!(
            validate_plan_paths(&root_writable, &ctx()),
            invalid("sandbox writable_roots must not include `/`")
        );
    }

    #[test]
    fn plan_paths_report_the_first_root_in_a_fixed_order() {
        let mut all_root = spec();
        all_root.workspace = p("/");
        all_root.sandbox.writable_roots = vec![p("/")];
        let root_ctx = LaunchContext {
            runtime_dir: p("/"),
            tmpdir: p("/"),
            config_dir: Some(p("/")),
            ..ctx()
        };
        assert_eq!(
            validate_plan_paths(&all_root, &root_ctx),
            invalid("workspace must not be `/`")
        );
        assert_eq!(
            validate_plan_paths(&spec(), &root_ctx),
            invalid("tmpdir must not be `/`")
        );
    }

    #[test]
    fn the_standard_session_kinds_are_accepted() {
        // Every kind the port has today is a standard one; the refusal covers a kind added
        // later, which can't be built here until it exists.
        for kind in [
            SessionKind::Home {},
            SessionKind::Task {
                task_id: PlainId::try_from("t1").unwrap(),
            },
            SessionKind::OneOff {
                run_id: PlainId::try_from("r1").unwrap(),
            },
        ] {
            assert_eq!(validate_standard_session_kind(&kind), Ok(()), "{kind:?}");
        }
    }

    fn tools(names: &[&str]) -> Vec<ToolName> {
        names
            .iter()
            .map(|name| ToolName::try_from(*name).unwrap())
            .collect()
    }

    fn fake_capability_tools(cap: ToolCapability) -> Option<&'static [&'static str]> {
        match cap {
            ToolCapability::Read => Some(&["Read"]),
            ToolCapability::Search => Some(&["Glob", "Grep"]),
            ToolCapability::Edit => Some(&["Edit", "Write"]),
            _ => None,
        }
    }

    #[test]
    fn capability_allowlist_expands_tokens_in_order_without_duplicates() {
        let expanded = expand_capability_allowlist(
            &tools(&[
                "Grep",
                "cap:read",
                "cap:search",
                "Read",
                "mcp__office__send",
                "Grep",
            ]),
            fake_capability_tools,
        );
        assert_eq!(
            expanded,
            Ok(vec![
                "Grep".to_owned(),
                "Read".to_owned(),
                "Glob".to_owned(),
                "mcp__office__send".to_owned(),
            ])
        );
        assert_eq!(
            expand_capability_allowlist(&[], fake_capability_tools),
            Ok(Vec::new())
        );
    }

    #[test]
    fn capability_allowlist_refuses_unknown_or_unmapped_capabilities() {
        for token in ["cap:teleport", "cap:unknown", "cap:shell"] {
            assert_eq!(
                expand_capability_allowlist(&tools(&["Read", token]), fake_capability_tools),
                Err(PlanError::Unsupported("tool capability")),
                "{token}"
            );
        }
    }

    #[test]
    fn static_env_names_keep_their_order() {
        let names = static_env_names(&["B_TOKEN", "A_KEY"]);
        let names: Vec<&str> = names.iter().map(EnvName::as_str).collect();
        assert_eq!(names, ["B_TOKEN", "A_KEY"]);
        assert!(static_env_names(&[]).is_empty());
    }

    #[test]
    fn runtime_env_add_lists_fixed_switches_then_the_config_dir() {
        let fixed = [("Z_DISABLE", "1"), ("A_SCRUB", "true")];
        let as_strs = |env: &[(EnvName, String)]| -> Vec<(String, String)> {
            env.iter()
                .map(|(name, value)| (name.as_str().to_owned(), value.clone()))
                .collect()
        };

        let with_config = runtime_env_add(&fixed, "RUNTIME_CONFIG_DIR", &ctx());
        assert_eq!(
            as_strs(&with_config),
            [
                ("Z_DISABLE".to_owned(), "1".to_owned()),
                ("A_SCRUB".to_owned(), "true".to_owned()),
                (
                    "RUNTIME_CONFIG_DIR".to_owned(),
                    "/office/agents/a1/config".to_owned()
                ),
            ]
        );

        let without_config = LaunchContext {
            config_dir: None,
            ..ctx()
        };
        let env = runtime_env_add(&fixed, "RUNTIME_CONFIG_DIR", &without_config);
        assert_eq!(
            as_strs(&env),
            [
                ("Z_DISABLE".to_owned(), "1".to_owned()),
                ("A_SCRUB".to_owned(), "true".to_owned()),
            ]
        );
    }
}
