/* ---------------------------------------------------------------- data */
const M = {
  you:   { name: "You", kind: "person", role: "Owner", owner: true, teams: [], mgr: null, live: "available", task: "", proj: null },
  tess: { name: "Tess", kind: "agent", tier: "builtin", builtin: true, role: "Chief of Staff", teams: ["leadership"], mgr: "you", live: "available", task: "Synthesizing executive brief & unblocking queues", proj: null, goal: "Tactical Execution & Squad Supervisor — synthesizes live operations into the Executive Brief, surfaces risks early, and queues actionable proposals for the owner.", setup: { runtime: "Built-in", model: "Chief of Staff (Built-in AI)", how: "On demand & scheduled brief", office: "hq", home: "system/tess" } },
  maya:  { name: "Maya Chen", kind: "person", role: "Operations", teams: ["leadership"], mgr: "you", live: "offline", task: "", proj: null, last: "Signed out · last seen yesterday" },
  ada:   { name: "Ada", kind: "agent", tier: "lead", role: "Engineering Manager", teams: ["leadership"], mgr: "you", live: "working", task: "Re-sequence the nav slices for IA v2", proj: "agency-v1", goal: "Every T-A70 slice re-planned against IA v2, with owners and order", setup: { runtime: "Claude Code", model: "Claude Opus", how: "Always on", office: "hq", home: "projects/ac/agents/ac-staff-engineer-ada" } },
  rowan: { name: "Rowan", kind: "agent", tier: "lead", role: "Staff Engineer", teams: ["platform"], mgr: "ada", live: "working", task: "Custody lock fix ruling", proj: "office-federation", goal: "A written ruling on the custody lock with the tests that pin it", setup: { runtime: "Claude Code", model: "Claude Opus", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-fullstack-engineer-rowan" } },
  eden:  { name: "Eden Lin", kind: "agent", tier: "lead", role: "Product Designer", teams: ["product"], mgr: "you", live: "working", task: "Presence vocabulary across screens", proj: "look-feel", goal: "One presence set used in roster, chat, team and profile", setup: { runtime: "Claude Code", model: "Claude Opus", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-product-designer-eden" } },
  emery: { name: "Emery Park", kind: "agent", tier: "lead", role: "Product Manager", teams: ["product"], mgr: "ada", live: "working", task: "Information architecture v2 prototype", proj: "look-feel", goal: "A clickable IA v2 prototype the owner can iterate on", setup: { runtime: "Claude Code", model: "Claude Opus", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-product-manager-emery" } },
  tomas: { name: "Tomas", kind: "agent", tier: "specialist", role: "Code Reviewer", teams: ["quality"], mgr: "ada", live: "working", task: "Review of Sage's lock fix 492e59e4", proj: "agency-v1", goal: "A READY or CHANGES NEEDED verdict on 492e59e4", setup: { runtime: "Claude Code", model: "Claude Opus", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-code-reviewer-tomas" } },
  kai:   { name: "Kai", kind: "agent", tier: "specialist", role: "Rust Developer", teams: ["platform"], mgr: "ada", live: "withyou", task: "Codec split 2a.3.2b", proj: "agency-v1", goal: "2a.3.2b merged into v1 with the full suite green", setup: { runtime: "Codex", model: "gpt-6.1-sol, high effort", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-software-engineer-kai" } },
  sage:  { name: "Sage", kind: "agent", tier: "specialist", role: "Rust Developer", teams: ["platform"], mgr: "ada", live: "working", task: "Custody lock fix", proj: "office-federation", goal: "Lock fix merged after the owner picks lease or per-office lock", setup: { runtime: "Codex", model: "gpt-6.1-sol, high effort", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-software-engineer-sage" } },
  quinn: { name: "Quinn", kind: "agent", tier: "specialist", role: "Rust Developer", teams: ["platform"], mgr: "ada", live: "asleep", task: "", proj: "office-federation", setup: { runtime: "Codex", model: "gpt-6.1-sol, high effort", how: "On a schedule (weekdays 09:00)", office: "sat", home: "projects/ac/agents/ac-software-engineer-quinn" } },
  jules: { name: "Jules", kind: "agent", tier: "specialist", role: "Svelte Developer", teams: ["app"], mgr: "ada", live: "asleep", budgetCapped: true, task: "Search placeholder contrast (T-A84)", proj: "look-feel", goal: "T-A84 fixed with an e2e test that fails before the fix", setup: { runtime: "Codex", model: "gpt-6.1-sol, high effort", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-software-engineer-jules" } },
  avery: { name: "Avery", kind: "agent", tier: "specialist", role: "Svelte Developer", teams: ["app"], mgr: "ada", live: "available", paused: true, task: "", proj: "look-feel", setup: { runtime: "Codex", model: "gpt-6.1-sol, high effort", how: "Wakes when called", office: "hq", home: "projects/ac/agents/ac-software-engineer-avery" } },
  noor:  { name: "Noor Haddad", kind: "agent", tier: "specialist", role: "Support Engineer", teams: ["quality"], mgr: "tomas", live: "asleep", task: "", proj: null, setup: { runtime: "Claude Code", model: "Claude Sonnet", how: "Wakes when called", office: null, home: "projects/ac/agents/ac-support-engineer-noor", needs: "not placed in an office" } }
};
const TEAMS = {
  leadership: { name: "Leadership", lead: "maya", purpose: "Sets direction, approves hires, releases and decisions that cross teams.", members: ["maya", "ada"], channel: "general" },
  product:    { name: "Product & Design", lead: "eden", purpose: "Decides what we build and how it looks and behaves.", members: ["eden", "emery"], channel: "design" },
  platform:   { name: "Platform (Rust)", lead: "rowan", purpose: "Builds the office server, sync and the core the app runs on.", members: ["rowan", "kai", "sage", "quinn"], channel: "platform" },
  app:        { name: "App (Svelte)", lead: "jules", purpose: "Builds the desktop app the owner uses every day.", members: ["jules", "avery"], channel: "agency-v1" },
  quality:    { name: "Quality", lead: "tomas", purpose: "Reviews every branch before it merges and keeps the bar high.", members: ["tomas", "noor"], channel: "agency-v1" }
};
const P = {
  "agency-v1":         { name: "Agentic Org v1.0", release: "v1.0", health: "At risk", upd: 12, lead: "ada", teams: ["leadership", "platform", "app", "quality"], goal: "Ship the first release of Agentic Org: hire agents, chat, projects and work, on one office.", updated: "12m ago", channel: "agency-v1", due: "Dec 5", dueMonth: "Dec", dueDay: "05", pace: "+2d behind", paceTone: "warn", blocker: "2 sign-offs waiting: v1.0 release & test access", unblockSpotId: "in1", unblockHref: "#/inbox/in1", unblockLabel: "Review (2)", milestoneTitle: "Agentic Org v1.0", horizonStatus: "2 sign-offs", horizonNote: "Release & test access sign-offs", risk: "Two updates wait on approval and local test access is waiting on your permission." },
  "office-federation": { name: "Office federation", release: "v0.9", health: "Off track", upd: 25, lead: "rowan", teams: ["platform"], goal: "Run agents in a second office that syncs with HQ.", updated: "25m ago", channel: "platform", due: "Nov 26", dueMonth: "Nov", dueDay: "26", pace: "+5d behind", paceTone: "bad", blocker: "File-access choice waiting 25m · Build box office offline", unblockSpotId: "in2", unblockHref: "#/inbox/in2", unblockLabel: "Decide", milestoneTitle: "Office sync test", horizonStatus: "Needs you", horizonNote: "Rowan · Office federation", risk: "The Build box office stayed offline after sleep. Sage's fix waits on your verification, and sync can't resume until the office is back online." },
  "look-feel":         { name: "Look & feel refresh", release: "v1.1", health: "On track", upd: 60, lead: "eden", teams: ["product", "app"], goal: "One calm, consistent look across every screen, with icons and no emoji.", updated: "1h ago", channel: "design", due: "Nov 28", dueMonth: "Nov", dueDay: "28", pace: "1d ahead", paceTone: "ok", blocker: "", clearNote: "All checks passing · projected Nov 27 (1d early)", unblockSpotId: "in17", unblockHref: "#/inbox/in17", milestoneTitle: "Look & feel refresh", horizonStatus: "1d ahead", horizonNote: "Final design polish sign-off", risk: "" },
  "proto-agency":      { name: "Early UI prototype", upd: 99999, lead: null, teams: ["product"], goal: "Explore the look of the app before v1.", updated: "Nov 2", channel: "design", archived: true }
};
const HEALTH = { "On track": { tone: "green", icon: "check-circle" }, "At risk": { tone: "red", icon: "warn" }, "Off track": { tone: "red", icon: "x-circle" } };
const W = [
  { id: "T-A112", kind: "task", title: "Re-sequence T-A70 nav slices for IA v2",
    description: "Reorder the IA v2 navigation work so the page structure lands before the visual polish. The revised sequence gives each owner a dependency they can verify before handing off their slice.",
    status: "Done", closedDaysAgo: 2, requestedBy: "you", proj: "agency-v1", who: "ada", pri: "High",
    criteria: [
      { text: "Sequence the slices", done: true },
      { text: "Assign owners", done: true },
      { text: "Review dependencies", done: true },
      { text: "Publish the plan", done: true }
    ]
  },
  { id: "T-A118", kind: "task", title: "Custody lock fix",
    description: "Keep a single writer in charge when an office goes offline during a custody transfer. Verify the lock clears after the office reconnects so work can resume without a manual reset. The release decision is waiting in Inbox.",
    status: "Blocked", waitsOn: "you", requestedBy: "you", proj: "office-federation", who: "sage", pri: "High", etaHours: 6,
    criteria: [
      { text: "Prevent a second office from taking the same lock", done: true },
      { text: "Verify recovery after reconnecting", done: false }
    ]
  },
  { id: "T-A121", kind: "task", title: "Split codec 2a.3.2b",
    description: "Split the 2a.3.2b codec into encoder and decoder changes so each can be reviewed on its own. Keep the existing wire format and confirm that a satellite on the previous version can still sync.\n\nReview the shared fixtures before requesting the release sign-off.",
    status: "In progress", requestedBy: "ada", proj: "agency-v1", who: "kai", pri: "Medium", etaHours: 2,
    materializedFromIdeation: true,
    ideationComments: [
      { id: "ic-121-1", who: "tess", at: Date.now() - 86400000, text: "Routed owner intake to @Ada (Engineering Manager) and @Rowan (Staff Engineer): “Break up the huge 2a.3.2b codec change so reviews don't stall v1.0.”" },
      { id: "ic-121-2", who: "rowan", at: Date.now() - 82800000, text: "We debated splitting by message type vs encoder/decoder boundary. Encoder vs decoder keeps wire compatibility testable on its own without duplicating fixtures." },
      { id: "ic-121-3", who: "ada", at: Date.now() - 79200000, text: "Agreed — encoder/decoder split is cleanest. Materializing into the final spec and assigning @Kai." },
    ],
    criteria: [
      { text: "Split encoders", done: true },
      { text: "Split decoders", done: false },
      { text: "Run compatibility tests", done: false },
      { text: "Review fixtures", done: false }
    ]
  },
  { id: "T-A124", kind: "task", title: "CSP gates in the UI suite",
    description: "Add regression checks for the UI’s Content Security Policy across the browser suite. Inventory the existing fixtures, confirm which local test permissions the runner needs, then run the suite against the app build.\n\nThe suite is waiting on approval for the local test gateway.",
    status: "Blocked", waitsOn: "you", requestedBy: "you", proj: "agency-v1", who: "jules", pri: "High", etaHours: 8,
    subtasks: [
      { id: "T-A147", title: "Inventory CSP fixtures",
        description: "Compare the current browser fixtures against the policies the app serves. Record which directives are already covered and identify the fixtures the suite still needs.",
        status: "Done", pri: "Medium", criteria: [
        { text: "List covered policies", done: true }, { text: "Record fixture gaps", done: true }
      ] },
      { id: "T-A148", title: "Verify browser test permissions",
        description: "Confirm the runner can reach the local test gateway using the test profile. Record the permissions the browser suite needs so the parent task can proceed after approval.",
        status: "Blocked", pri: "High", criteria: [
        { text: "Identify required permissions", done: true }, { text: "Verify permissions in the test runner", done: false }
      ] },
      { id: "T-A149", title: "Run the CSP gate suite",
        description: "Run the UI policy checks once the environment is ready. Save the results and separately flag any failures that need to be reproduced before the release gate can clear.",
        status: "To do", pri: "Medium", etaHours: 2, criteria: [
        { text: "Run browser checks", done: false }, { text: "Record results", done: false }
      ] },
    ]
  },
  { id: "T-A130", kind: "task", title: "Presence vocabulary across screens",
    description: "Use the same presence words wherever members appear in the UI. Cover active, paused and offline states in the directory, org chart and conversation list, with text that works alongside each icon.",
    status: "In progress", proj: "look-feel", who: "eden", pri: "Medium", etaHours: 3,
    criteria: [
      { text: "Agree on presence labels", done: true },
      { text: "Use the labels in the directory, org chart and Chat", done: false }
    ]
  },
  { id: "T-A131", kind: "task", title: "Information architecture v2 prototype",
    description: "Build the clickable IA v2 prototype around the daily workflow. Make it easy to open an Inbox item, find its task and navigate back with the keyboard.\n\nThe primary routes are linked. Record a walkthrough of the remaining cases before the design handoff.",
    status: "In progress", requestedBy: "you", proj: "look-feel", who: "emery", pri: "High", etaHours: 5,
    criteria: [
      { text: "Map primary routes", done: true },
      { text: "Link prototype views", done: true },
      { text: "Review navigation", done: true },
      { text: "Record the walkthrough", done: false }
    ]
  },
  { id: "T-A133", kind: "task", title: "Restyle status pills",
    description: "Update status pills to use the shared semantic colors and typography. Check that the label stays readable in both compact rows and narrow layouts.",
    status: "Done", closedDaysAgo: 1, proj: "look-feel", who: "avery", pri: "Low",
    criteria: [
      { text: "Apply the shared status colors", done: true },
      { text: "Verify labels on a narrow screen", done: true }
    ]
  },
  { id: "T-A135", kind: "task", title: "HQ to satellite snapshot sync",
    description: "Send a small snapshot from HQ to the satellite office and verify that the replay produces the same records. Keep this as the first end-to-end check once the build box comes back online.",
    status: "To do", requestedBy: "you", proj: "office-federation", who: "quinn", pri: "Medium", etaHours: 12,
    criteria: [
      { text: "Create a snapshot", done: false },
      { text: "Transfer to the satellite", done: false },
      { text: "Verify replay", done: false }
    ]
  },
  { id: "T-A138", kind: "task", title: "Enrollment certificates for new offices",
    description: "Set up enrollment certificates for the new offices. Verify that a newly enrolled office can connect to HQ and that a revoked enrollment is refused.",
    status: "In progress", proj: "office-federation", who: "rowan", pri: "Low", etaHours: 10,
    criteria: [
      { text: "Enroll a new office with a valid certificate", done: true },
      { text: "Verify revoked enrollment is refused", done: false }
    ]
  },
  { id: "T-A140", kind: "task", title: "Review Sage's lock fix 492e59e4",
    description: "Review the custody lock change at revision 492e59e4. Check the lease-release path and leave reproducible notes for Sage on any findings that affect the handoff.",
    status: "Done", closedDaysAgo: 3, requestedBy: "you", proj: "agency-v1", who: "tomas", pri: "High",
    criteria: [
      { text: "Review how the lease is released", done: true },
      { text: "Leave handoff notes for Sage", done: true }
    ]
  },
  { id: "T-A142", kind: "task", title: "Write the v1.0 release notes",
    description: "Write the v1.0 release notes for the people installing and operating an office. Call out the sync changes, the new checks and any steps the reader needs to take before upgrading.",
    status: "Done", closedDaysAgo: 4, proj: "agency-v1", who: "you", pri: "Medium",
    criteria: [
      { text: "Summarize the sync changes and release checks", done: true },
      { text: "Document the upgrade steps", done: true }
    ]
  },
  { id: "T-A144", kind: "task", title: "Onboarding checklist for new people",
    description: "Give new people a short checklist for joining an office, finding their team and opening their first task. Include the links they need to finish setup without waiting for an introduction.",
    status: "Done", closedDaysAgo: 5, requestedBy: "you", proj: "agency-v1", who: "maya", pri: "Low",
    criteria: [
      { text: "Link the team directory and first task", done: true },
      { text: "Walk through the checklist with a new member", done: true }
    ]
  },
  { id: "T-A146", kind: "task", title: "Sign the Apple developer agreement for notarization",
    description: "Complete the Apple Developer agreement required for notarization. Confirm the agreement version in the developer account, then identify the account holder who can sign it.\n\nKeep the release gate open until the signed agreement is confirmed.",
    status: "To do", requestedBy: "you", proj: "agency-v1", who: "ada", pri: "High",
    materializedFromIdeation: true,
    ideationComments: [
      { id: "ic-146-1", who: "tess", at: Date.now() - 172800000, text: "Routed owner intake to @Ada: “We need Apple notarization unblocked before cutting the macOS v1.0 installer.”" },
      { id: "ic-146-2", who: "ada", at: Date.now() - 169200000, text: "Checked notarization logs — it's blocked on the updated Apple Developer Program License Agreement, not a signing cert bug. Splitting into version check (T-A150) and account-holder check (T-A151)." },
    ],
    subtasks: [
      { id: "T-A150", title: "Confirm the agreement version",
        description: "Match the agreement version in the Apple Developer account to the one listed on the notarization requirement. Record the result for the signer.",
        status: "Done", pri: "Low", criteria: [
        { text: "Match the agreement to the developer account", done: true }
      ] },
      { id: "T-A151", title: "Check who can sign",
        description: "Confirm who holds signing authority for the developer account and share that contact with the release team.",
        status: "In progress", pri: "High", etaHours: 1.5, criteria: [
        { text: "Find the account holder", done: true }, { text: "Confirm signing authority", done: false }
      ] },
    ]
  },
  { id: "T-A70", kind: "bug", title: "Landing page hides what needs you",
    description: "The landing page pushed items needing a decision below the first screen. Verify that urgent requests are visible when the briefing opens, including at shorter viewport heights.",
    status: "Fixed", closedDaysAgo: 18, requestedBy: "you", proj: "agency-v1", who: "ada", pri: "Major",
    criteria: [
      { text: "Keep urgent decisions visible on the first screen", done: true },
      { text: "Verify the briefing on short viewports", done: true }
    ]
  },
  { id: "T-A76", kind: "bug", title: "Status pill truncates letter by letter at 390 px",
    description: "At a 390-pixel viewport, the status label could wrap one letter at a time. Verify the label keeps a readable shape beside a long work item title.",
    status: "Fixed", closedDaysAgo: 14, proj: "look-feel", who: "avery", pri: "Minor",
    criteria: [
      { text: "Reproduce the label wrapping at 390 pixels", done: true },
      { text: "Verify the label beside a long title", done: true }
    ]
  },
  { id: "T-A81", kind: "bug", title: "Satellite stays Offline after the Mac wakes",
    description: "The satellite stayed offline in the UI after the Mac woke up. Verify that the connection recovers and the presence status updates after sleep.",
    status: "Fixed", closedDaysAgo: 21, requestedBy: "you", proj: "office-federation", who: "sage", pri: "Critical",
    criteria: [
      { text: "Reconnect the satellite after the Mac wakes", done: true },
      { text: "Refresh presence after reconnecting", done: true }
    ]
  },
  { id: "T-A84", kind: "bug", title: "Search placeholder fails contrast",
    description: "The search placeholder is hard to read in some themes. Compare it against the field background in unfocused and focused states, including the contrast setting.",
    status: "In progress", pipeline: "ci", proj: "look-feel", who: "jules", pri: "Minor",
    criteria: [
      { text: "Record the theme and field state where contrast fails", done: true },
      { text: "Verify placeholder contrast in every theme", done: false }
    ]
  },
  { id: "T-A86", kind: "bug", title: "Chat scrolls to top after sending",
    description: "After sending a message, Chat jumped to the oldest entries. Keep focus in the composer and bring the newly sent message into view.",
    status: "Fixed", closedDaysAgo: 16, proj: "agency-v1", who: "jules", pri: "Major",
    criteria: [
      { text: "Keep focus in the composer after sending", done: true },
      { text: "Scroll to the newest message", done: true }
    ]
  },
  { id: "T-A88", kind: "bug", title: "Inbox loses replies when the office restarts",
    description: "A reply recorded in Inbox could disappear after an office restart. Confirm that the thread still shows the saved reply and that the request keeps the correct state after reconnecting.",
    status: "New", proj: "agency-v1", who: null, pri: "Critical",
    criteria: [
      { text: "Record a reply and restart the office", done: false },
      { text: "Verify the reply and request status survive", done: false }
    ]
  },
  { id: "T-A9",  kind: "request", title: "Read-only view of an agent's journal and memory",
    description: "Provide a read-only view of an agent’s journal and remembered context so an owner can understand recent decisions before assigning work.",
    status: "Approved", closedDaysAgo: 4, proj: "agency-v1", who: "emery", pri: "by Emery" },
  { id: "T-A11", kind: "request", title: "Invite flow for a second office",
    description: "Explore the invitation flow for someone joining a second office. Show which office they are joining and what happens before their first sync completes.",
    status: "Rowan is scoping", proj: "office-federation", who: "rowan", pri: "Sent Nov 9",
    criteria: [
      { text: "Map the steps for joining the second office", done: true },
      { text: "Explain when the first sync is ready", done: false }
    ]
  },
  { id: "T-A12", kind: "request", title: "Export activity as CSV",
    description: "Let an owner export the current Activity view to CSV for monthly reporting. Include timestamps, people, projects and event types, and apply the filters visible on the page.",
    status: "Approved", closedDaysAgo: 2, proj: "agency-v1", who: "maya", pri: "From Maya Chen",
    criteria: [
      { text: "Agree on the CSV columns", done: true },
      { text: "Export the filtered Activity view", done: false }
    ]
  },
  { id: "T-A7",  kind: "request", title: "Dark-only look for v1.0",
    description: "Record the initial decision to begin with a dark theme for v1.0 so the team can focus on the core workflows during the first implementation pass.",
    status: "Approved", closedDaysAgo: 15, proj: "look-feel", who: "eden", pri: "Approved Nov 10" }
];
const WORK_DISCUSSIONS = {
  "T-A12": {
    events: [
      ["check-circle", "Maya’s export request was approved", "Yesterday"],
      ["clipboard", "Maya requested monthly Activity exports", "2d"],
    ],
    comments: [["maya", "The export should reflect the date range and filters on screen. I’ll use it to reconcile the monthly report.", "Yesterday"]],
  },
  "T-A84": {
    events: [
      ["flag", "Tess proposed reassigning the contrast fix after Jules hit the budget cap", "4m"],
      ["clipboard", "Jules recorded the affected themes", "2h"],
    ],
    comments: [["jules", "Start with the keyboard-focused search field in Ocean and Cozy. The placeholder is the hardest to see there.", "2h"]],
  },
  "T-A112": { events: [
    ["check-circle", "Ada published the revised sequence and assigned owners", "Yesterday"],
    ["clipboard", "Ada linked T-A70 to the navigation work", "Yesterday"],
  ] },
  "T-A118": {
    events: [
      ["chat", "Rowan shared the custody lock ruling in #platform", "25m"],
      ["pause", "Sage paused the fix pending the file-access decision", "26m"],
    ],
    comments: [["rowan", "Automatic expiry gives us a recovery path if the satellite disappears mid-transfer. Keep the reconnect check in the handoff.", "25m"]],
  },
  "T-A121": {
    events: [
      ["terminal", "Kai finished the codec run. All 412 checks passed.", "12m"],
      ["check-circle", "Tomas marked the review ready", "14m"],
      ["flag", "Ada approved splitting the change into two slices", "20m"],
    ],
    comments: [
      ["tomas", "The encoder changes are ready. Please keep the decoder compatibility fixtures with the second slice so we can review them together.", "14m"],
      ["kai", "The run is green. I’ve linked the release sign-off so the owner can approve it from Inbox.", "12m"],
    ],
  },
  "T-A124": {
    events: [
      ["pause", "Jules moved the task to Blocked while the local test permission is pending", "3h"],
      ["check-circle", "Jules finished the fixture inventory in T-A147", "4h"],
      ["clipboard", "Ada assigned the browser checks to Jules", "Yesterday"],
    ],
    comments: [
      ["ada", "Please keep the three browser checks under this task so we can see which check is holding up the release.", "Yesterday"],
      ["jules", "The fixture inventory is complete. I’m waiting for the testing approval before running the suite.", "3h"],
    ],
  },
  "T-A131": {
    events: [
      ["flag", "Emery sent draft 4.1 for design review", "Yesterday"],
      ["check-circle", "Emery finished the navigation pass", "Yesterday"],
    ],
    comments: [["eden", "The route hierarchy looks right. Include a keyboard-only pass from Inbox to a task in the walkthrough.", "Yesterday"]],
  },
  "T-A135": { events: [
    ["pause", "Quinn’s run paused while the build box is offline", "4h"],
    ["clipboard", "Rowan queued the snapshot check for the second office", "Yesterday"],
  ] },
  "T-A146": {
    events: [
      ["check-circle", "Ada confirmed the agreement version in T-A150", "2h"],
      ["clipboard", "Ada added a signing-authority check in T-A151", "Yesterday"],
    ],
    comments: [["ada", "The agreement version is confirmed. I’m checking who can sign before I mark the release gate ready.", "2h"]],
  },
  "T-A147": { events: [["check-circle", "Jules finished the inventory and documented the fixture gaps", "4h"]] },
  "T-A148": { events: [["pause", "Jules requested local test access in Inbox", "3h"]] },
  "T-A150": { events: [["check-circle", "Ada matched the agreement to the developer account", "2h"]] },
};
const KIND = {
  approval:   { label: "Approval", plural: "Approvals", icon: "check-circle", clears: "Only when you approve or deny" },
  suggestion: { label: "Suggestion", plural: "Suggestions", icon: "sparkle", clears: "When you apply or dismiss" },
  question:   { label: "Question", plural: "Questions", icon: "question", clears: "When you answer" },
  decision:   { label: "Decision", plural: "Decisions", icon: "flag", clears: "When you choose an option" },
  problem:    { label: "Problem", plural: "Problems", icon: "warn", clears: "When it's resolved" }
};
const RECOMMENDATION_TONES = {
  ok: ["check-circle", "Low risk"],
  warn: ["warn", "Investigate"],
  bad: ["shield-alert", "Use caution"]
};
const IN = [
  { id: "in1", kind: "approval", sub: "Release sign-off", from: "ada", time: "12m", group: "Today", what: "Approve publishing Kai's sync update to v1.0", cta: "Approve Kai's sync", why: "Kai's update makes office syncing cleaner and more reliable for v1.0. All 412 automated checks passed and Tomas approved review with zero open issues. High-impact release changes require your final sign-off before publishing.", audit: ["412 automated checks passing", "Tomas reviewed — 0 open issues", "Reversible release gate · T-A121"], recTone: "ok", rec: "Ada recommends: approve. Tomas completed review with no open issues.", impact: [["shield-check", "ok", "Low risk", "Easy to roll back"], ["bolt", "", "High impact", "Unblocks v1.0 launch"], ["check-circle", "ok", "Checks passed", "412 automated checks"]], proj: "agency-v1", task: "T-A121", expires: "Expires in 4 hours (release window)" },
  { id: "in18", kind: "suggestion", sub: "Tess suggestion", from: "tess", time: "4m", group: "Today", what: "Reroute Jules's 3 queued tasks to Maya & Rowan", cta: "Apply Jules reroute", why: "Jules reached its $210 monthly budget cap and 3 assigned tasks are sitting stalled in the queue. Maya and Rowan have available capacity in the same repositories with zero context-switch overhead and zero added spend.", audit: ["Maya & Rowan capacity verified", "$0 added monthly spend", "Recovers ~6h of queued wait · T-A84"], recTone: "ok", rec: "Tess suggests: apply reroute. Recovers ~6h of queued wait time at $0 extra spend.", impact: [["shield-check", "ok", "Zero added spend", "Stays under $210 cap"], ["bolt", "", "Recovers ~6h wait", "Unblocks 3 queued tasks"], ["check-circle", "ok", "Capacity verified", "Maya & Rowan available"]], proj: "look-feel", task: "T-A84", expires: "",
    acts: [["Apply Jules reroute", "Rerouted 3 tasks to Maya & Rowan", "sparkle"], ["Dismiss", "Suggestion dismissed", "x", "deny"]], clears: "When you apply or dismiss Tess's suggestion" },
  { id: "in5", kind: "approval", sub: "Access request", from: "jules", time: "3h", group: "Today", what: "Allow local test access for Jules's security checks", cta: "Allow test access", why: "Jules needs temporary permission to run security checks against the local test server only. No live customer or company data is exposed, and only the owner can grant local access permissions.", audit: ["Scoped to local test sandbox only", "Zero production or customer data", "Auto-expires after security run · T-A124"], recTone: "ok", rec: "Jules recommends: allow for testing only.", impact: [["shield-check", "ok", "Low risk", "Test sandbox only"], ["bolt", "", "High impact", "Unblocks v1.0 security"], ["lock", "", "Limited scope", "No live data access"]], proj: "agency-v1", task: "T-A124", expires: "Expires tomorrow" },
  { id: "in2", kind: "decision", sub: "Direction needed", from: "sage", time: "25m", group: "Today", what: "Choose how offices coordinate shared files: timed access or fixed lock", why: "Work on second-office syncing is paused until you pick. Automatic timed access recovers cleanly if an office goes offline; a fixed per-office lock is a day faster to finish.", audit: ["Rowan & Sage aligned on timed access", "Auto-recovers from office disconnects", "Unblocks Office federation · T-A118"], recTone: "warn", rec: "Sage recommends timed access. Rowan agrees.", impact: [["shield-alert", "warn", "Moderate risk", "Harder to change later"], ["bolt", "warn", "High impact", "Unblocks Office sync"], ["scale", "", "Safety vs. speed", "Auto-recovers · +1 day"]], proj: "office-federation", task: "T-A118", opts: ["Use timed access", "Fixed office lock"], expires: "" },
  { id: "in16", kind: "approval", sub: "Company standard", from: "rowan", time: "45m", group: "Today", what: "Require automated checks before every release?", cta: "Adopt CI standard", why: "Agentic Org v1.0 and Look & feel refresh both cut revision rates by running automated checks before handoff. Adopting this company-wide means every new project enforces the same quality bar automatically.", audit: ["Proven across 2 active projects", "Overridable per project anytime", "Keeps first-try approval >80% · T-A121"], recTone: "ok", rec: "Rowan & Ada recommend: adopt standard. Keeps first-try approval rates above 80%.", impact: [["shield-check", "ok", "Low risk", "Proven across 2 projects"], ["sparkle", "", "Org-wide impact", "Applies to future projects"], ["adjustments", "muted", "Reversible", "Override per project"]], proj: "agency-v1", task: "T-A121", expires: "",
    acts: [["Adopt CI standard", "Adopted as company standard", "sparkle"], ["Keep project-only", "Kept project-only", "x", "deny"]], clears: "When you adopt or keep it project-only" },
  { id: "in17", kind: "approval", sub: "Design sign-off", from: "emery", time: "1h", group: "Today", what: "Approve final design polish for Look & feel refresh", cta: "Approve design polish", why: "Half of the v1.1 refresh is already shipped (3 of 6 tasks done, 1 day ahead). Approving the final design polish unlocks the remaining 3 screens for the Nov 28 milestone, well within your monthly budget.", audit: ["Eden & Emery design review passed", "3 of 6 milestone tasks shipped", "1 day ahead of Nov 28 target · T-A131"], recTone: "ok", rec: "Emery & Eden recommend: approve. All design and budget checks passed.", impact: [["shield-check", "ok", "Low risk", "Visual polish only"], ["bolt", "", "Medium impact", "Unlocks final 3 screens"], ["calendar-check", "ok", "1 day ahead", "3 of 6 tasks done"]], proj: "look-feel", task: "T-A131", expires: "",
    acts: [["Approve design polish", "Design polish approved", "check"], ["Request changes", "Changes requested", "pencil", "changes"]], clears: "When you approve or request changes" },
  { id: "in3", kind: "question", sub: "Question", from: "eden", time: "1h", group: "Today", what: "Should the org chart show people and agents in one tree?", why: "Presence vocabulary (T-A130). Splitting them makes reporting lines harder to follow; one tree needs the kind word on every node.", recTone: "warn", rec: "Eden recommends: one tree, with the kind word on each node.", proj: "look-feel", task: "T-A130", expires: "" },
  { id: "in4", kind: "approval", sub: "Hire request", from: "ada", time: "2h", group: "Today", what: "Hire a QA engineer for the Quality team", why: "Tomas reviews every branch and UI validation queues behind him: 6 branches are waiting today.", recTone: "warn", rec: "ada recommends: one agent, Playwright focus, reports to Tomas.", proj: "agency-v1", task: "", expires: "" },
  { id: "in11", kind: "approval", sub: "Fix to verify", from: "sage", time: "3h", group: "Today", what: "Verify fix: T-A81", why: "Sage fixed the satellite staying Offline after the Mac wakes. Tomas's review is READY and the wake test passes on a test satellite. Approving closes the bug; reopening sends it back to Sage.", recTone: "ok", rec: "Sage recommends: approve the fix.", proj: "office-federation", task: "T-A81", expires: "",
    acts: [["Approve fix", "Fix approved", "check"], ["Reopen", "Reopened", "refresh"]], sets: { "Fix approved": "Closed", "Reopened": "Triaged" }, clears: "When you approve the fix or reopen the bug" },
  { id: "in12", kind: "problem", sub: "Budget reached", from: "jules", time: "3h", group: "Today", what: "Budget reached for this month", why: "Jules has used this month's model budget. New runs wait until December 1, or until you raise the budget in Agent setup.", rec: "", proj: "look-feel", task: "", expires: "",
    acts: [["Dismiss for this month", "Dismissed for this month", "x"]], src: { href: "#/settings/agents/jules", label: "Open Agent setup" }, clears: "When you dismiss it for this month, or the budget resets" },
  { id: "in13", kind: "problem", sub: "Unplaced agent", from: "noor", time: "5h", group: "Today", what: "Noor Haddad isn't placed in an office yet", why: "Noor was hired yesterday, but no office runs Noor yet, so Noor can't pick up work. Choose an office in Agent setup.", rec: "", proj: "agency-v1", task: "", expires: "",
    src: { href: "#/settings/agents/noor", label: "Open Agent setup" }, clears: "When Noor is placed in an office" },
  { id: "in9", kind: "problem", sub: "Goal paused", from: "quinn", time: "4h", group: "Today", what: "Quinn's goal is paused: the Build box office is offline", why: "T-A135 can't start until the satellite office is back. Resume the goal once it's online, or move T-A135 to HQ.", rec: "", proj: "office-federation", task: "T-A135", expires: "", resume: true, clears: "When the goal resumes" },
  { id: "in6", kind: "approval", sub: "Review requested", from: "emery", time: "Yesterday", group: "Earlier", what: "Review T-A131: the IA v2 prototype, draft 4.1", why: "Emery applied Eden's draft-4 recheck and asks for your review before the stories go to engineering.", recTone: "ok", rec: "Eden recommends: approve. Her recheck findings are fixed.", proj: "look-feel", task: "T-A131", expires: "",
    acts: [["Approve", "Approved", "check"], ["Request changes", "Changes requested", "pencil", "changes"]], clears: "When you answer, here or anywhere" },
  { id: "in14", kind: "approval", sub: "Hand-off", from: "ada", time: "1h", group: "Today", what: "Take on T-A146: sign the Apple developer agreement", why: "Notarizing the v1.0 build needs the account holder to accept Apple's updated agreement. Only you can sign it, so ada handed T-A146 to you.", rec: "", proj: "agency-v1", task: "T-A146", expires: "",
    acts: [["Accept", "Accepted", "check"]], assignTo: { "Accepted": "you" }, clears: "When you accept it, here or in Chat" },
  { id: "in7", kind: "question", sub: "Question", from: "rowan", time: "Yesterday", group: "Earlier", what: "Keep the old codec behind a flag for one release?", why: "Removing it now saves 400 lines; keeping it lets a satellite on the old version still sync.", recTone: "warn", rec: "Rowan recommends: keep it for one release.", proj: "agency-v1", task: "T-A121", expires: "" },
  { id: "in15", kind: "problem", sub: "Critical bug", from: "tomas", time: "2h", group: "Today", what: "Critical bug: T-A88, the Inbox loses replies when the office restarts", why: "Tomas reproduced it twice: a reply sent while the office restarts never reaches the agent. Nobody is assigned yet.", recTone: "bad", rec: "Tomas recommends: assign it to Rowan.", proj: "agency-v1", task: "T-A88", expires: "", assign: true, clears: "When it's assigned to anyone, or closed" },
  { id: "in10", kind: "problem", sub: "Crash", from: "rowan", time: "Yesterday", group: "Earlier", what: "A run on T-A138 crashed", why: "The run stopped with an error after 3 minutes: the certificate tool wasn't found on PATH. Nothing was changed.", rec: "", proj: "office-federation", task: "T-A138", expires: "", dismiss: true, clears: "When you dismiss it, or the next run succeeds" },
  { id: "in8", kind: "approval", sub: "Run approval", from: "rowan", time: "Nov 10", group: "Earlier", what: "Approve the A-OC1 pins", why: "Pins the office contract versions for v1.0.", rec: "", proj: "office-federation", task: "", expires: "", handled: true, outcome: "Approved" }
];
const A = [
  { id: "a1", type: "run", who: "kai", time: "12m", proj: "agency-v1", text: "finished a run on T-A121 Split codec 2a.3.2b", summary: "Woken by ada's message · 12 min · finished", log: [["09:29", "Woken by ada's message in #agency-v1"], ["09:30", "Read T-A121 and Tomas's review notes"], ["09:33", "Changed 3 files in org-sync-proto"], ["09:38", "Ran the Rust suite: 412 passed"], ["09:40", "Pushed 0a520e5b and asked Tomas for review"], ["09:41", "Finished · 12 min"]] },
  { id: "a2", type: "work", who: "tomas", time: "14m", proj: "agency-v1", text: "marked the review of 0a520e5b READY" },
  { id: "a3", type: "decision", who: "ada", time: "20m", proj: "agency-v1", text: "approved splitting 2a.3.2b into two slices" },
  { id: "a4", type: "conversation", who: "rowan", time: "25m", proj: "office-federation", text: "posted the custody lock ruling in #platform" },
  { id: "a5", type: "run", who: "sage", time: "26m", proj: "office-federation", text: "stopped a run on T-A118 Custody lock fix", summary: "Woken on schedule · 6 min · stopped, needs you", log: [["09:12", "Woken on schedule"], ["09:13", "Read Rowan's ruling"], ["09:17", "Two options left; asked you to decide"], ["09:18", "Stopped · waiting on you"]] },
  { id: "a13", type: "org", who: "ada", time: "40m", proj: "agency-v1", text: "asked to hire a QA engineer for Quality" },
  { id: "a6", type: "work", who: "jules", time: "3h", proj: "agency-v1", text: "moved T-A124 CSP gates to Blocked" },
  { id: "a7", type: "conversation", who: "eden", time: "1h", proj: "look-feel", text: "asked you about the org chart tree" },
  { id: "a8", type: "run", who: "tomas", time: "1h", proj: "agency-v1", text: "finished a review run on 492e59e4", summary: "Woken by Sage's request · 18 min · finished", log: [["08:20", "Woken by Sage's request"], ["08:21", "Checked out 492e59e4 in a clean worktree"], ["08:30", "Ran clippy and the workspace tests"], ["08:38", "Verdict: CHANGES NEEDED, 2 findings"]] },
  { id: "a9", type: "work", who: "avery", time: "2h", proj: "look-feel", text: "marked T-A133 Restyle status pills Done" },
  { id: "a14", type: "org", who: "you", time: "2h", proj: "look-feel", text: "paused Avery" },
  { id: "a10", type: "decision", who: "you", time: "Nov 10", proj: "office-federation", text: "approved the A-OC1 pins" },
  { id: "a11", type: "work", who: "emery", time: "Nov 10", proj: "look-feel", text: "created T-A131 Information architecture v2 prototype" },
  { id: "a12", type: "conversation", who: "maya", time: "Nov 10", proj: "agency-v1", text: "asked for an activity export (T-A12)" }
];
const CH = {
  "general":   { name: "general", kind: "channel", topic: "Everyone in Agentic Org", msgs: [
    { id: "g1", who: "maya", time: "Nov 10, 16:02", text: "Could we export the activity feed for the monthly report? Filed as T-A12." },
    { id: "g2", who: "you", time: "Nov 10, 16:20", text: "Yes, later. Leaving it unrouted for now." } ] },
  "agency-v1": { name: "agency-v1", kind: "channel", topic: "Agency v1.0 delivery", proj: "agency-v1", msgs: [
    { id: "m1", who: "ada", time: "09:12", text: "Morning. Codec split is in review; CSP gates next. I am re-planning the nav slices for IA v2 today." },
    { id: "m2", who: "tomas", time: "09:40", text: "READY on Kai's 0a520e5b. It needs the owner's go to merge (tier A).", thread: [
      { who: "kai", time: "09:41", text: "Thanks. I'll merge as soon as it's approved." },
      { who: "ada", time: "09:43", text: "Approved on my side. Waiting on the owner." },
      { who: "tomas", time: "09:44", text: "Noted. Next up is Sage's 492e59e4." } ] },
    { id: "m3", who: "jules", time: "09:52", text: "The CSP gateway test can't reach the gateway without a local permission.", ask: { id: "ask1", inbox: "in5", q: "Allow localhost:8443 for the dev profile?", opts: ["Allow", "Not now", "Ask why"] } },
    { id: "m4", who: "you", time: "10:01", text: "Looking at it now." } ] },
  "platform":  { name: "platform", kind: "channel", topic: "Office server, sync and core", proj: "office-federation", msgs: [
    { id: "p1", who: "rowan", time: "09:10", text: "Ruling on the custody lock: the lease is safer, the per-office lock is faster. Owner decides; details in T-A118." },
    { id: "p2", who: "sage", time: "09:18", text: "Waiting until the owner picks one. Asked in the Inbox." } ] },
  "design":    { name: "design", kind: "channel", topic: "Look, feel and information architecture", proj: "look-feel", msgs: [
    { id: "d1", who: "emery", time: "08:55", text: "IA v2 draft 4.1 is up as a clickable prototype, with Eden's design system v2 applied. Feedback welcome." },
    { id: "d2", who: "eden", time: "09:05", text: "Presence set looks right. One open question on the org chart tree; I asked the owner." } ] },
  "dm-ada":    { name: "ada", kind: "dm", with: "ada", msgs: [
    { id: "x1", who: "ada", time: "08:40", text: "Heads up: Agency v1.0 is at risk by about 3 days unless the CSP gate unblocks today." },
    { id: "x2", who: "you", time: "08:45", text: "Understood. I'll look at Jules's ask this morning." } ] },
  "dm-eden":   { name: "Eden", kind: "dm", with: "eden", msgs: [
    { id: "y1", who: "eden", time: "09:06", text: "Quick one in your Inbox about the org chart, whenever you have a minute." } ] },
  "dm-maya":   { name: "Maya Chen", kind: "dm", with: "maya", msgs: [
    { id: "z1", who: "maya", time: "Nov 10", text: "I'll draft the onboarding checklist this week (T-A144)." } ] },
  "a2a-kai-rowan": { name: "Kai and Rowan", kind: "a2a", pair: ["kai", "rowan"], msgs: [
    { id: "k1", who: "kai", time: "09:20", text: "The codec frame header changes size in 2a.3.2b. Does the satellite need to read both for a release?" },
    { id: "k2", who: "rowan", time: "09:24", text: "Yes, one release. Keep the old decoder behind a flag; I'll ask the owner if it's worth the 400 lines." },
    { id: "k3", who: "kai", time: "09:25", text: "Thanks. Keeping the flag for now while you ask the owner." } ] },
  "a2a-tomas-sage": { name: "Tomas and Sage", kind: "a2a", pair: ["tomas", "sage"], msgs: [
    { id: "s1", who: "tomas", time: "08:38", text: "Two findings on 492e59e4: the lease renewal can race with shutdown, and the test doesn't fail without the fix." },
    { id: "s2", who: "sage", time: "08:50", text: "Agreed on both. Waiting on the owner's lease decision before I fix the first." } ] }
};
const OFFICES = {
  hq:  { name: "HQ", machine: "This Mac", status: "Online", tone: "green", icon: "check-circle", note: "Main office · version 1.0.0-rc.3" },
  sat: { name: "Build box", machine: "mac-mini-2 (satellite)", status: "Offline", tone: "outline", icon: "offline", note: "Last seen 2 hours ago · syncs with HQ" }
};
const NOTES = {
  default: (id) => ({
    journal: [
      { d: "2026-11-18", items: [M[id].task ? "Working on: " + M[id].task + "." : "No task picked up today.", "Synced with my team's backlog and journals before starting."] },
      { d: "2026-11-17", items: ["Closed out yesterday's review follow-ups and updated local tests."] }
    ],
    memory: {
      decisions: [
        "Reports to " + (M[M[id].mgr] ? M[M[id].mgr].name : "the owner") + "; escalates owner-only approvals and decisions through the Inbox.",
        "Follows the architecture and delivery rules in RULES.md."
      ],
      context: [
        M[id].proj ? "Primary project: " + P[M[id].proj].name + "." : "Not assigned to a primary project yet.",
        "Office: " + officeName(M[id].setup.office) + " · runs via " + M[id].setup.runtime + "."
      ]
    }
  }),
  ada: {
    journal: [
      { d: "2026-11-18", items: ["Re-sequenced T-A70 nav slices against IA v2 draft 4.1.", "Reviewed Kai's codec split 0a520e5b with Tomas; escalated Tier-A merge approval to the owner.", "Handed T-A146 (Apple developer agreement) to the owner since only the account holder can sign."] },
      { d: "2026-11-17", items: ["Flagged Agency v1.0 as At risk (3-day slip) pending CSP gateway permission.", "Drafted QA engineer hire request for Quality team to clear Tomas's 6-branch review queue."] }
    ],
    memory: {
      decisions: [
        "Tier-A merges (sync protocol, custody locks, auth) always require owner approval before landing on main.",
        "Unassigned critical bugs (like T-A88) must be triaged to an owner within one cycle."
      ],
      context: [
        "Agency v1.0 target ship date is Dec 5, 2026; primary bottleneck is review throughput behind Tomas.",
        "Emery and Eden own IA v2 and design system v2 alignment before engineering starts T-A70 slices."
      ]
    }
  },
  emery: {
    journal: [
      { d: "2026-11-18", items: ["Unified the Agent Console into the Agent Profile with the Neural Orb, Context Window, and 7-tab structure.", "Applied Eden's design system v2 to IA v2 draft 4.1."] },
      { d: "2026-11-17", items: ["Completed inventory of every emoji in the app, with Heroicon replacements."] }
    ],
    memory: {
      decisions: [
        "The owner wants the app organized like a company: what needs you first, then projects and work, then teams, then settings.",
        "No emoji anywhere in the system; icons always carry a clear label."
      ],
      context: [
        "Agents have a unified profile (with live Orb, Context, Tasks, Sessions, Journal, Memory, Knowledge, and Settings) plus an org-wide table in Organization settings."
      ]
    }
  }
};
const KNOWLEDGE = {
  default: (id) => {
    const m = M[id], p = m.proj ? P[m.proj].name : "Organization";
    return [
      { id: "k1", folder: "rules", title: "Operating manual & delivery rules", path: `${m.setup.home}/CLAUDE.md`, updated: "Today", summary: `Role boundaries, goal creation via CreateGoal, peer sync loop, and subagent briefing conventions for ${m.name}.`, body: `Loaded at session start for ${m.name} (${m.role}).\n• Owns end-to-end delivery for assigned tasks in ${p}.\n• Creates a checkable completion condition before starting implementation.\n• Updates tasks/BACKLOG.md, journal/, and memory/MEMORY.md before ending a session.` },
      { id: "k2", folder: "specs", title: `${p} project brief`, path: `projects/${m.proj || "org"}/CLAUDE.md`, updated: "Yesterday", summary: `Shared goals, constraints, milestones, and team roster for ${p}.`, body: `${m.proj ? P[m.proj].goal : "Organization-wide shared brief."}\n\nIncludes active milestone deadlines, office topology constraints, and cross-team ownership.` }
    ];
  },
  ada: [
    { id: "k1", folder: "adrs", title: "ADR-014 · Tiered merge risk & owner gates", path: "projects/ac/agents/ac-staff-engineer-ada/knowledge/adrs/tier-a-merge-gates.md", updated: "Today", summary: "Defines Risk Tier A/B/C criteria and when an approval card is routed to the owner's Inbox.", body: "Tier A changes touch sync framing, custody locks, enrollment certificates, or permission boundaries. They require:\n1. Tomas's READY review verdict.\n2. Full workspace suite green on the merge commit.\n3. Explicit owner approval via Inbox before fast-forward." },
    { id: "k2", folder: "specs", title: "T-A70 · IA v2 navigation slice plan", path: "projects/ac/agents/ac-staff-engineer-ada/knowledge/specs/t-a70-slices.md", updated: "Today", summary: "Sequenced engineering slices mapping Emery's IA v2 prototype onto the Svelte desktop app.", body: "Slice 1: Unified sidebar + Home Needs You cards.\nSlice 2: Agent Profile with live Orb, Context Window, and 7-tab workspace.\nSlice 3: Work List/Board toggle and Inbox kind filters." },
    { id: "k3", folder: "rules", title: "Architecture rules (coupling & boundaries)", path: "docs/architecture-rules.md", updated: "Nov 10", summary: "Shared engineering invariants enforced across every Rust and Svelte pull request.", body: "Core principles:\n• Dependency direction flows inward toward domain invariants.\n• Agents coordinate via shared contracts and peer journals, never editing another agent's home directory." }
  ],
  eden: [
    { id: "k1", folder: "designs", title: "Presence vocabulary & status tokens", path: "projects/ac/agents/ac-product-designer-eden/knowledge/designs/presence-vocabulary.md", updated: "Today", summary: "Canonical states (Needs you, With you, Working, Paused, Available, Asleep, Offline) across roster, chat, and profile.", body: "Every presence state uses one color token, one icon, and one label. Never combine raw emoji with state indicators." },
    { id: "k2", folder: "designs", title: "Design system v2 · surface & hairline spec", path: "projects/ac/agents/ac-product-designer-eden/knowledge/designs/design-system-v2.md", updated: "Yesterday", summary: "Dark gray-800 canvas, hairline dividers (--line / --line2), and calm indigo/sky telemetry accents.", body: "Avoid competing dark-navy sub-surfaces. Secondary rails use rgb(0 0 0 / .08) with a 1px hairline separator so telemetry and Orb glow integrate naturally." }
  ]
};
const PAST_SESSIONS = [
  { id: "s-0941", label: "Current live session", when: "Started 09:12 · Active", runtime: "hq", summary: "Working through active tasks, live tool calls, and owner messages", active: true },
  { id: "s-0814", label: "Morning peer sync & triage", when: "Today 08:14 · 19m", runtime: "hq", summary: "Synced peer journals, updated backlog priorities, 14 tool calls" },
  { id: "s-1640", label: "Yesterday afternoon delivery run", when: "Yesterday 16:40 · 28m", runtime: "hq", summary: "Completed review follow-ups and committed clean test assertions" }
];

const STATUSES = { task: ["To do", "In progress", "Blocked", "Done"], bug: ["New", "Triaged", "In progress", "Fixed", "Closed"], request: ["Not routed", "Rowan is scoping", "Scoped", "Approved"] };
const ALL_STATUSES = [...new Set([...STATUSES.task, ...STATUSES.bug, ...STATUSES.request, ...W.map((w) => w.status)])];
/* Calendar (prototype mock). Today is Thu, Nov 26 2026. Each event has a date, time range, attendees
   (people and agents) and optionally a project or team; personal events have neither. */
const CAL_TODAY = "2026-11-26";
const ev = (id, date, start, end, title, attendees, extra = {}) => ({ id, date, start, end, title, attendees, ...extra });
const EVENTS = [
  ev("ev10", "2026-11-23", "09:30", "10:00", "Platform standup", ["rowan", "kai", "sage", "quinn"], { team: "platform" }),
  ev("ev11", "2026-11-24", "13:00", "14:00", "IA v2 critique", ["you", "eden", "emery"], { proj: "look-feel" }),
  ev("ev12", "2026-11-25", "10:00", "11:00", "Office sync incident review", ["you", "rowan", "sage"], { proj: "office-federation" }),
  ev("ev18", "2026-11-25", "16:00", "16:30", "Quality triage", ["tomas", "noor"], { team: "quality" }),
  ev("ev13", "2026-11-26", "09:30", "10:00", "Platform standup", ["rowan", "kai", "sage", "quinn"], { team: "platform" }),
  ev("ev14", "2026-11-26", "15:00", "15:30", "1:1 with Ada", ["you", "ada"]),
  ev("ev1", "2026-11-27", "10:00", "10:30", "Release sign-off review", ["you", "ada", "tomas", "kai"], { proj: "agency-v1" }),
  ev("ev15", "2026-11-27", "14:00", "15:00", "Quality bar sync", ["tomas", "noor", "ada"], { team: "quality" }),
  ev("ev2", "2026-11-28", "14:00", "14:45", "Design polish walkthrough", ["you", "eden", "emery", "avery"], { proj: "look-feel" }),
  ev("ev16", "2026-11-30", "09:30", "10:00", "Platform standup", ["rowan", "kai", "sage", "quinn"], { team: "platform" }),
  ev("ev3", "2026-12-01", "11:00", "12:00", "Office sync test", ["you", "rowan", "sage", "quinn"], { proj: "office-federation" }),
  ev("ev17", "2026-12-03", "13:00", "14:00", "App (Svelte) demo", ["you", "jules", "avery", "eden"], { team: "app" }),
  ev("ev19", "2026-12-03", "16:00", "16:30", "Budget review", ["you", "maya"]),
  ev("ev4", "2026-12-04", "16:00", "16:30", "v1.0 launch go / no-go", ["you", "maya", "ada", "emery", "tomas", "rowan"], { proj: "agency-v1" }),
];


const OPEN = (w) => !["Done", "Fixed", "Closed", "Approved", "Declined"].includes(w.status);
const NEEDS_ME_SUBS = ["Review requested", "Hand-off"];
const blockedOnOwner = (w) => w.status === "Blocked" && w.waitsOn === "you";
const needsMe = (w) => OPEN(w) && (blockedOnOwner(w) || IN.some((i) => !i.handled && i.task === w.id && NEEDS_ME_SUBS.includes(i.sub)));

const officeName = (o) => (OFFICES[o] ? OFFICES[o].name : "Not placed");

export {
  A,
  ALL_STATUSES,
  CAL_TODAY,
  CH,
  EVENTS,
  HEALTH,
  IN,
  KIND,
  KNOWLEDGE,
  M,
  NOTES,
  OFFICES,
  OPEN,
  P,
  PAST_SESSIONS,
  RECOMMENDATION_TONES,
  STATUSES,
  TEAMS,
  W,
  WORK_DISCUSSIONS,
  blockedOnOwner,
  needsMe,
  officeName,
};
