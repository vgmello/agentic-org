//! Declarative vendor profiles and stateless [`AgentRuntime`] adapters (`agy`, `gemini`, `kimi`, `pi`).

use org_agent_runtime::transcript::TranscriptParser;
use org_agent_runtime::{
    AgentRuntime, Capabilities, CostBasis, ImageInputMode, LaunchContext, LaunchPlan,
    MidTurnDelivery, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun,
    RuntimeHostProfile, RuntimeId, Session, SessionSpec, UsageScope, VersionRange,
};

use crate::plan::{build_headless_plan, build_take_over_plan};
use crate::probe::{parse_probe, plan_probe};
use crate::profile::{
    AcpProfile, AuthProbeStrategy, ExpectedMode, ModeMapping, StateDirStrategy,
    SystemPromptDelivery, TakeOverStrategy,
};
use crate::session::AcpSession;
use crate::transcript::{
    AGY_TRANSCRIPT_PARSER, GEMINI_TRANSCRIPT_PARSER, KIMI_TRANSCRIPT_PARSER, PI_TRANSCRIPT_PARSER,
};

/// Host credential paths inside `<gemini-dir>` that must remain denied to tools (`S2`).
pub const AGY_CONFIG_CREDENTIAL_FILES: &[&str] = &[
    "antigravity-cli/antigravity-oauth-token",
    "oauth_creds.json",
    "google_accounts.json",
];

/// Host credential paths inside `~/.gemini` denied for Gemini CLI sessions (`S2`).
pub const GEMINI_CONFIG_CREDENTIAL_FILES: &[&str] = &[
    "oauth_creds.json",
    "google_accounts.json",
    "mcp-oauth-tokens.json",
];

/// Additional `$HOME`-relative credential paths denied for Gemini CLI sessions (`S2`).
pub const GEMINI_HOST_CREDENTIAL_FILES: &[&str] = &[
    ".config/gemini/oauth_creds.json",
    ".config/gemini/google_accounts.json",
    ".config/gemini/credentials.json",
];

/// Host credential paths inside `~/.kimi` denied for Kimi Code CLI sessions (`S2`).
pub const KIMI_CONFIG_CREDENTIAL_FILES: &[&str] =
    &["credentials.json", "auth.json", "config.toml", "oauth.json"];

/// Additional `$HOME`-relative credential paths denied for Kimi Code CLI sessions (`S2`).
pub const KIMI_HOST_CREDENTIAL_FILES: &[&str] =
    &[".config/kimi/credentials.json", ".config/kimi/config.toml"];

/// Host credential paths inside `~/.pi` denied for Pi ACP sessions (`S2`).
pub const PI_CONFIG_CREDENTIAL_FILES: &[&str] =
    &["agent/auth.json", "auth.json", "credentials.json"];

/// Additional `$HOME`-relative credential paths denied for Pi ACP sessions (`S2`).
pub const PI_HOST_CREDENTIAL_FILES: &[&str] =
    &[".config/pi/auth.json", ".config/pi/credentials.json"];

const STANDARD_ACP_MODES: ModeMapping = ModeMapping {
    ask: ExpectedMode {
        cli_flag: None,
        reported_name: "default",
    },
    accept_edits: ExpectedMode {
        cli_flag: Some("accept-edits"),
        reported_name: "accept-edits",
    },
    plan: ExpectedMode {
        cli_flag: Some("plan"),
        reported_name: "plan",
    },
};

const GEMINI_ACP_MODES: ModeMapping = ModeMapping {
    ask: ExpectedMode {
        cli_flag: None,
        reported_name: "default",
    },
    accept_edits: ExpectedMode {
        cli_flag: Some("auto_edit"),
        reported_name: "auto_edit",
    },
    plan: ExpectedMode {
        cli_flag: Some("plan"),
        reported_name: "plan",
    },
};

/// Declarative ACP profile for Google Antigravity (`agy acp --stdio`, verified on `1.5.16–1.5.17`).
pub static AGY_PROFILE: AcpProfile = AcpProfile {
    id: "agy",
    display_name: "Agy",
    binary_name: "agy",
    aliases: &["google", "antigravity", "antigravity-cli"],
    headless_base_args: &[
        "acp",
        "--stdio",
        "--include-default-customizations=false",
        "--disable-telemetry",
    ],
    sandbox_flag: Some("--sandbox"),
    state_dir: StateDirStrategy::AgyGeminiDir {
        isolated_rel: "gemini",
        app_data_rel: "antigravity-cli",
    },
    read_only_dir_flag: Some("--add-dir"),
    system_prompt: SystemPromptDelivery::MetaAntigravityAppend,
    resume_method: "session/resume",
    modes: STANDARD_ACP_MODES,
    take_over: TakeOverStrategy::AgyConversation,
    env_add: &[
        ("AGY_DISABLE_AUTOUPDATE", "1"),
        ("AGY_SUBPROCESS_ENV_SCRUB", "1"),
    ],
    probe_env_add: &[("AGY_DISABLE_AUTOUPDATE", "1")],
    config_dir_env: "AGY_CONFIG_DIR",
    auth_env_names: &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "ANTIGRAVITY_API_KEY",
        "AGY_AUTH_TOKEN",
        "GOOGLE_GEMINI_BASE_URL",
    ],
    secret_env_names: &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "ANTIGRAVITY_API_KEY",
        "AGY_AUTH_TOKEN",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "AGY_SESSION_TOKEN",
    ],
    endpoint_env_var: Some("GOOGLE_GEMINI_BASE_URL"),
    subscription_auth_env_var: "AGY_AUTH_TOKEN",
    api_key_auth_env_var: "GEMINI_API_KEY",
    host_profile: RuntimeHostProfile {
        host_config_subdir: Some(".gemini"),
        config_credential_files: AGY_CONFIG_CREDENTIAL_FILES,
        host_credential_files: &[],
        uses_host_oauth_token_injection: false,
        uses_mount_mask_sandbox: false,
        needs_explicit_sandbox_ro_dirs: true,
        sandbox_settings_file: None,
    },
    auth_probe: AuthProbeStrategy::OneShot {
        args: &["models"],
        method: "agy-cli",
    },
    tested_versions: ("1.5.16", "1.5.17"),
    tested_semver_range: Some(((1, 5, 16), (1, 5, 17))),
    stderr_error_prefix: Some("AGY_ERROR:"),
    transcript_parser: &AGY_TRANSCRIPT_PARSER,
};

/// Declarative ACP profile for Google Gemini CLI (`gemini --acp`).
pub static GEMINI_PROFILE: AcpProfile = AcpProfile {
    id: "gemini",
    display_name: "Gemini",
    binary_name: "gemini",
    aliases: &["gemini-cli"],
    headless_base_args: &["--acp"],
    sandbox_flag: Some("--sandbox"),
    state_dir: StateDirStrategy::None,
    read_only_dir_flag: Some("--include-directories"),
    system_prompt: SystemPromptDelivery::PlannedFileEnv {
        rel_path: "gemini-system.md",
        env_var: "GEMINI_SYSTEM_MD",
    },
    resume_method: "session/load",
    modes: GEMINI_ACP_MODES,
    take_over: TakeOverStrategy::Unsupported,
    env_add: &[("GEMINI_TELEMETRY_ENABLED", "false"), ("NO_COLOR", "1")],
    probe_env_add: &[("GEMINI_TELEMETRY_ENABLED", "false"), ("NO_COLOR", "1")],
    config_dir_env: "GEMINI_CLI_HOME",
    auth_env_names: &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_OAUTH_TOKEN",
        "GOOGLE_GEMINI_BASE_URL",
    ],
    secret_env_names: &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_OAUTH_TOKEN",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ],
    endpoint_env_var: Some("GOOGLE_GEMINI_BASE_URL"),
    subscription_auth_env_var: "GEMINI_OAUTH_TOKEN",
    api_key_auth_env_var: "GEMINI_API_KEY",
    host_profile: RuntimeHostProfile {
        host_config_subdir: Some(".gemini"),
        config_credential_files: GEMINI_CONFIG_CREDENTIAL_FILES,
        host_credential_files: GEMINI_HOST_CREDENTIAL_FILES,
        uses_host_oauth_token_injection: false,
        uses_mount_mask_sandbox: false,
        needs_explicit_sandbox_ro_dirs: false,
        sandbox_settings_file: None,
    },
    auth_probe: AuthProbeStrategy::AcpInitialize {
        method: "gemini-acp",
    },
    tested_versions: ("0.1.0", "99.0.0"),
    tested_semver_range: Some(((0, 1, 0), (99, 0, 0))),
    stderr_error_prefix: None,
    transcript_parser: &GEMINI_TRANSCRIPT_PARSER,
};

/// Declarative ACP profile for Moonshot Kimi Code CLI (`kimi acp`).
pub static KIMI_PROFILE: AcpProfile = AcpProfile {
    id: "kimi",
    display_name: "Kimi",
    binary_name: "kimi",
    aliases: &["kimi-cli", "kimi-code", "moonshot"],
    headless_base_args: &["acp"],
    sandbox_flag: None,
    state_dir: StateDirStrategy::None,
    read_only_dir_flag: None,
    system_prompt: SystemPromptDelivery::FirstTurnPromptBlock,
    resume_method: "session/load",
    modes: STANDARD_ACP_MODES,
    take_over: TakeOverStrategy::Unsupported,
    env_add: &[("NO_COLOR", "1")],
    probe_env_add: &[("NO_COLOR", "1")],
    config_dir_env: "KIMI_SHARE_DIR",
    auth_env_names: &[
        "KIMI_API_KEY",
        "MOONSHOT_API_KEY",
        "KIMI_OAUTH_TOKEN",
        "KIMI_BASE_URL",
    ],
    secret_env_names: &[
        "KIMI_API_KEY",
        "MOONSHOT_API_KEY",
        "KIMI_OAUTH_TOKEN",
        "KIMI_SESSION_TOKEN",
    ],
    endpoint_env_var: Some("KIMI_BASE_URL"),
    subscription_auth_env_var: "KIMI_OAUTH_TOKEN",
    api_key_auth_env_var: "KIMI_API_KEY",
    host_profile: RuntimeHostProfile {
        host_config_subdir: Some(".kimi"),
        config_credential_files: KIMI_CONFIG_CREDENTIAL_FILES,
        host_credential_files: KIMI_HOST_CREDENTIAL_FILES,
        uses_host_oauth_token_injection: false,
        uses_mount_mask_sandbox: false,
        needs_explicit_sandbox_ro_dirs: false,
        sandbox_settings_file: None,
    },
    auth_probe: AuthProbeStrategy::AcpInitialize { method: "kimi-acp" },
    tested_versions: ("0.1.0", "99.0.0"),
    tested_semver_range: Some(((0, 1, 0), (99, 0, 0))),
    stderr_error_prefix: None,
    transcript_parser: &KIMI_TRANSCRIPT_PARSER,
};

/// Declarative ACP profile for Pi (`pi-acp`).
pub static PI_PROFILE: AcpProfile = AcpProfile {
    id: "pi",
    display_name: "Pi",
    binary_name: "pi-acp",
    aliases: &["pi-acp", "pi-coding-agent"],
    headless_base_args: &[],
    sandbox_flag: None,
    state_dir: StateDirStrategy::None,
    read_only_dir_flag: None,
    system_prompt: SystemPromptDelivery::FirstTurnPromptBlock,
    resume_method: "session/load",
    modes: STANDARD_ACP_MODES,
    take_over: TakeOverStrategy::Unsupported,
    env_add: &[("NO_COLOR", "1")],
    probe_env_add: &[("NO_COLOR", "1")],
    config_dir_env: "PI_CODING_AGENT_DIR",
    auth_env_names: &["PI_API_KEY", "PI_OAUTH_TOKEN", "PI_BASE_URL"],
    secret_env_names: &["PI_API_KEY", "PI_OAUTH_TOKEN", "PI_SESSION_TOKEN"],
    endpoint_env_var: Some("PI_BASE_URL"),
    subscription_auth_env_var: "PI_OAUTH_TOKEN",
    api_key_auth_env_var: "PI_API_KEY",
    host_profile: RuntimeHostProfile {
        host_config_subdir: Some(".pi"),
        config_credential_files: PI_CONFIG_CREDENTIAL_FILES,
        host_credential_files: PI_HOST_CREDENTIAL_FILES,
        uses_host_oauth_token_injection: false,
        uses_mount_mask_sandbox: false,
        needs_explicit_sandbox_ro_dirs: false,
        sandbox_settings_file: None,
    },
    auth_probe: AuthProbeStrategy::AcpInitialize { method: "pi-acp" },
    tested_versions: ("0.1.0", "99.0.0"),
    tested_semver_range: Some(((0, 1, 0), (99, 0, 0))),
    stderr_error_prefix: None,
    transcript_parser: &PI_TRANSCRIPT_PARSER,
};

fn profile_capabilities(profile: &'static AcpProfile) -> Capabilities {
    Capabilities {
        mid_turn_delivery: MidTurnDelivery::NativeSteer,
        image_input: ImageInputMode::StagedFileOnly,
        interrupt_in_band: true,
        idle_interrupt_ends_turn: false,
        approval_bridge: true,
        // ACP `session/request_permission` selects an optionId ("allow"/"deny"); no free-form
        // deny reason string is carried in the ACP Selected outcome.
        deny_message_reaches_model: false,
        runtime_cancels_approvals: true,
        unsolicited_turns: false,
        session_id_known_up_front: false,
        resume_ignores_cwd: false,
        resume_applies_instructions: true,
        reads_workspace_instruction_files: false,
        tools_detach_from_group: true,
        take_over: !matches!(profile.take_over, TakeOverStrategy::Unsupported),
        // Enforcement flags remain false until live-binary confinement tests prove every
        // guarantee, keeping the office approval-bridge fallback active (B2).
        enforces_writable_roots: false,
        enforces_deny_write: false,
        enforces_deny_read: false,
        enforces_unix_socket_allowlist: false,
        enforces_tool_allowlist: false,
        enforces_deny_rules: false,
        usage_scope: UsageScope::TurnDelta,
        cost: CostBasis::None,
        tested_versions: VersionRange {
            min: profile.tested_versions.0.to_owned(),
            max: profile.tested_versions.1.to_owned(),
        },
    }
}

fn plan_profile_session(
    profile: &'static AcpProfile,
    spec: &SessionSpec,
    ctx: &LaunchContext,
) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
    let caps = profile_capabilities(profile);
    let (plan, cfg) = build_headless_plan(profile, spec, ctx, &caps)?;
    Ok((plan, Box::new(AcpSession::new(cfg))))
}

fn plan_profile_take_over(
    profile: &'static AcpProfile,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    cols: u16,
    rows: u16,
) -> Result<LaunchPlan, PlanError> {
    let caps = profile_capabilities(profile);
    build_take_over_plan(profile, spec, ctx, &caps, cols, rows)
}

macro_rules! define_acp_runtime {
    ($doc:literal, $struct_name:ident, $profile_static:ident) => {
        #[doc = $doc]
        #[derive(Debug, Default, Clone, Copy)]
        pub struct $struct_name;

        impl $struct_name {
            /// Creates a new stateless ACP runtime adapter.
            pub const fn new() -> Self {
                Self
            }

            /// Returns this adapter's declarative [`AcpProfile`].
            pub const fn profile(&self) -> &'static AcpProfile {
                &$profile_static
            }
        }

        impl AgentRuntime for $struct_name {
            fn id(&self) -> RuntimeId {
                RuntimeId::try_from($profile_static.id).expect("static ACP runtime id is valid")
            }

            fn binary_name(&self) -> &'static str {
                $profile_static.binary_name
            }

            fn capabilities(&self) -> Capabilities {
                profile_capabilities(&$profile_static)
            }

            fn plan_session(
                &self,
                spec: &SessionSpec,
                ctx: &LaunchContext,
            ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
                plan_profile_session(&$profile_static, spec, ctx)
            }

            fn plan_take_over(
                &self,
                spec: &SessionSpec,
                ctx: &LaunchContext,
                cols: u16,
                rows: u16,
            ) -> Result<LaunchPlan, PlanError> {
                plan_profile_take_over(&$profile_static, spec, ctx, cols, rows)
            }

            fn plan_probe(&self, probe: Probe, ctx: &ProbeContext) -> Option<ProbeRun> {
                plan_probe(&$profile_static, probe, ctx)
            }

            fn parse_probe(&self, probe: Probe, out: &ProbeOutput) -> ProbeResult {
                parse_probe(&$profile_static, probe, out)
            }

            fn aliases(&self) -> &'static [&'static str] {
                $profile_static.aliases
            }

            fn host_profile(&self) -> RuntimeHostProfile {
                $profile_static.host_profile
            }

            fn endpoint_env_var(&self) -> Option<&'static str> {
                $profile_static.endpoint_env_var
            }

            fn auth_env_var(&self, subscription_cli: bool, _secret: &str) -> Option<&'static str> {
                Some(if subscription_cli {
                    $profile_static.subscription_auth_env_var
                } else {
                    $profile_static.api_key_auth_env_var
                })
            }

            fn transcript_parser(&self) -> Option<&dyn TranscriptParser> {
                Some($profile_static.transcript_parser)
            }
        }
    };
}

define_acp_runtime!(
    "Stateless factory for Google Antigravity (`agy acp --stdio`) headless and take-over sessions.",
    AgyRuntime,
    AGY_PROFILE
);
define_acp_runtime!(
    "Stateless factory for Google Gemini CLI (`gemini --acp`) headless ACP sessions.",
    GeminiRuntime,
    GEMINI_PROFILE
);
define_acp_runtime!(
    "Stateless factory for Moonshot Kimi Code CLI (`kimi acp`) headless ACP sessions.",
    KimiRuntime,
    KIMI_PROFILE
);
define_acp_runtime!(
    "Stateless factory for Pi (`pi-acp`) headless ACP sessions.",
    PiRuntime,
    PI_PROFILE
);
