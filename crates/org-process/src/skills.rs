//! Built-in product skills served by `agent-os skill list` and `agent-os skill read`
//! (`doc-34` Decision 4, `doc-28` §3.5, `FC-111`).
//!
//! Each skill's body is a body-only Markdown file under `skills/<key>.md`, embedded at compile
//! time; the metadata stays here. The crate's tests check the budgets, the text hygiene and that
//! the files and this list match one to one.

/// Most bytes one skill body may take (4 KiB).
pub const SKILL_MAX_BYTES: usize = 4 * 1024;

/// High-level grouping of built-in product skills for `agent-os skill list --category`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillCategory {
    /// Day-1 operational workflow guides for all agents.
    Workflow,
    /// Version-locked product architecture and concept guides (Tier-3 knowledge).
    ProductGuide,
    /// Chief of Staff (Tess) operating recipes and presentation rules.
    ChiefOfStaff,
}

impl SkillCategory {
    /// Stable lowercase wire spelling (`"workflow"`, `"product_guide"`, `"chief_of_staff"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::ProductGuide => "product_guide",
            Self::ChiefOfStaff => "chief_of_staff",
        }
    }

    /// Parses a wire category spelling (trimmed, ASCII case-insensitive).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "workflow" => Some(Self::Workflow),
            "product_guide" => Some(Self::ProductGuide),
            "chief_of_staff" => Some(Self::ChiefOfStaff),
            _ => None,
        }
    }
}

/// One built-in product skill explaining how to operate inside Agentic Org.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinSkill {
    /// Stable lowercase skill key passed to `agent-os skill read --name <key>`; the body is
    /// `skills/<key>.md`.
    pub key: &'static str,
    /// Human-readable title.
    pub title: &'static str,
    /// Concise one-line description shown in `agent-os skill list`.
    pub description: &'static str,
    /// Origin label (`"built_in"`).
    pub source: &'static str,
    /// Category grouping (`Workflow`, `ProductGuide`, or `ChiefOfStaff`).
    pub category: SkillCategory,
    /// Markdown body returned by `agent-os skill read`.
    pub body_markdown: &'static str,
}

const fn skill(
    key: &'static str,
    title: &'static str,
    description: &'static str,
    category: SkillCategory,
    body_markdown: &'static str,
) -> BuiltinSkill {
    BuiltinSkill {
        key,
        title,
        description,
        source: "built_in",
        category,
        body_markdown,
    }
}

const BUILTIN_SKILLS: &[BuiltinSkill] = &[
    skill(
        "agent-os-basics",
        "Using agent-os",
        "CLI grammar, envelopes, schemas, dry runs.",
        SkillCategory::Workflow,
        include_str!("../skills/agent-os-basics.md"),
    ),
    skill(
        "tasks-create",
        "Creating tasks",
        "Search first, scope, criteria, and why.",
        SkillCategory::Workflow,
        include_str!("../skills/tasks-create.md"),
    ),
    skill(
        "tasks-execute",
        "Executing a task",
        "Deps, private planning, verified loops.",
        SkillCategory::Workflow,
        include_str!("../skills/tasks-execute.md"),
    ),
    skill(
        "tasks-finalize",
        "Finishing a task",
        "Verify criteria, checklist, docs, close.",
        SkillCategory::Workflow,
        include_str!("../skills/tasks-finalize.md"),
    ),
    skill(
        "messaging-and-asks",
        "Messages and structured asks",
        "Inbox, replies, handoffs, asks, snoozes.",
        SkillCategory::Workflow,
        include_str!("../skills/messaging-and-asks.md"),
    ),
    skill(
        "channels",
        "Channels",
        "Join, summarize, read, post, ask channels.",
        SkillCategory::Workflow,
        include_str!("../skills/channels.md"),
    ),
    skill(
        "code-and-worktrees",
        "Code, worktrees and pushes",
        "Prepare worktrees and publish branches.",
        SkillCategory::Workflow,
        include_str!("../skills/code-and-worktrees.md"),
    ),
    skill(
        "hiring-and-directory",
        "Directory and hiring",
        "People, teams, roles, and HR hiring.",
        SkillCategory::Workflow,
        include_str!("../skills/hiring-and-directory.md"),
    ),
    skill(
        "processes-and-standards",
        "Processes and standards",
        "Effective rules, precedence, and --why.",
        SkillCategory::Workflow,
        include_str!("../skills/processes-and-standards.md"),
    ),
    skill(
        "budgets",
        "Budgets",
        "Allowances, spend hygiene, budget holds.",
        SkillCategory::Workflow,
        include_str!("../skills/budgets.md"),
    ),
    skill(
        "errors-and-recovery",
        "Errors and recovery",
        "Error codes, retries, concurrency, stalls.",
        SkillCategory::Workflow,
        include_str!("../skills/errors-and-recovery.md"),
    ),
    skill(
        "batch-and-attach",
        "Batching with session attach",
        "Batch JSON commands on one connection.",
        SkillCategory::Workflow,
        include_str!("../skills/batch-and-attach.md"),
    ),
    skill(
        "product-org-and-teams",
        "Organizations, teams, roles, and hiring",
        "Orgs, teams, roles, hiring, caps.",
        SkillCategory::ProductGuide,
        include_str!("../skills/product-org-and-teams.md"),
    ),
    skill(
        "product-projects-and-work",
        "Projects, repositories, worktrees, and work",
        "Projects, repos, worktrees, tasks.",
        SkillCategory::ProductGuide,
        include_str!("../skills/product-projects-and-work.md"),
    ),
    skill(
        "product-processes-and-standards",
        "Processes and standards inheritance",
        "Inheritance chains, rules, --why.",
        SkillCategory::ProductGuide,
        include_str!("../skills/product-processes-and-standards.md"),
    ),
    skill(
        "product-offices-and-runtimes",
        "Offices, runtimes, model profiles, and provider pools",
        "Offices, runtimes, profiles, pools.",
        SkillCategory::ProductGuide,
        include_str!("../skills/product-offices-and-runtimes.md"),
    ),
    skill(
        "product-budgets-and-permissions",
        "Budgets, spend accounting, permissions, and sandboxes",
        "Budgets, holds, presets, sandbox.",
        SkillCategory::ProductGuide,
        include_str!("../skills/product-budgets-and-permissions.md"),
    ),
    skill(
        "product-troubleshooting",
        "Troubleshooting offices, sessions, limits, and recovery",
        "Offices, stalls, limits, recovery.",
        SkillCategory::ProductGuide,
        include_str!("../skills/product-troubleshooting.md"),
    ),
    skill(
        "tess-workflows",
        "Chief of Staff workflows",
        "Briefs, batching, proposals, UI.",
        SkillCategory::ChiefOfStaff,
        include_str!("../skills/tess-workflows.md"),
    ),
];

/// Returns all built-in product skills in stable order.
pub fn list_builtin_skills() -> &'static [BuiltinSkill] {
    BUILTIN_SKILLS
}

/// Returns built-in product skills filtered by optional [`SkillCategory`] in stable order.
pub fn list_builtin_skills_by_category(
    category: Option<SkillCategory>,
) -> Vec<&'static BuiltinSkill> {
    BUILTIN_SKILLS
        .iter()
        .filter(|s| category.is_none_or(|wanted| s.category == wanted))
        .collect()
}

/// Finds a built-in product skill by exact lowercase key (trimmed, case-insensitive lookup).
pub fn find_builtin_skill(name: &str) -> Option<&'static BuiltinSkill> {
    let needle = name.trim().to_ascii_lowercase();
    BUILTIN_SKILLS.iter().find(|s| s.key == needle)
}
