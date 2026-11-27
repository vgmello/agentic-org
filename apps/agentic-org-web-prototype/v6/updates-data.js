/* Updates (TASK-250): releases of Agentic Org, other instances on the same machines, the update history and each
   office's update log. Session-only mock data. Office versions and states live on OFFICES (data.js).
   Terms follow doc-48 §5 and doc-70 "Service and updates": a release pin is per instance, an upgrade switches a complete
   release, activation drains the instance, the prior release is kept, and a peer instance is never touched (SRV-16). */
import { OFFICES } from "./data.js";

const LATEST = "1.0.0";

/* level: the organization protocol level the release supports (doc-67 "Versions") */
const RELEASES = [
  { version: "1.0.0", date: "24 Nov 2026", id: "rel-2026.11.24-a7c3", sha: "9f2c41e07b3d", level: 4, kind: "level",
    summary: "First stable release. Office restart and updates from HQ, faster cross-office sync, and agent sandbox checks on Linux.",
    notes: ["Update offices from HQ: each office's instance downloads, verifies and restarts on the new release.", "Cross-office sync keeps a persistent connection, so changes from another office arrive in under a second.", "Agents on Linux offices run with the same Git-metadata and credential denies as on a Mac.", "Raises the organization level to 4. New features that need it wait until every active office runs 1.0.0."] },
  { version: "1.0.0-rc.4", date: "17 Nov 2026", id: "rel-2026.11.17-51be", sha: "41d8b0c2aa19", level: 3, kind: "security",
    summary: "Security fix: a browser session cookie could be sent to another instance on the same host. Also tightens the agent sandbox around Git metadata.",
    notes: ["Session proofs are now bound to the instance and origin, so a cookie sent to another port on the same host can't be used there.", "Agents can no longer read Git metadata outside their own task workspace.", "Logs redact provider tokens that appear in command output."] },
  { version: "1.0.0-rc.3", date: "6 Nov 2026", id: "rel-2026.11.06-c210", sha: "7a6e93d4f0c8", level: 3, kind: "",
    summary: "Linux offices: systemd user services and running in a container. Restart an office from HQ.",
    notes: ["Setup registers a systemd user service on Linux, or runs in the foreground inside a container.", "Restart office from HQ for an office that stopped.", "Office status shows why an office stopped."] },
  { version: "1.0.0-rc.2", date: "28 Oct 2026", id: "rel-2026.10.28-0e47", sha: "c3b1f5a90d22", level: 3, kind: "",
    summary: "Satellite offices: enroll from HQ's Add office, replicate the organization and relay messages between offices.",
    notes: ["Add office creates an invitation; the new office joins after an owner approves it.", "Messages and task changes reach every office through HQ."] },
];
const releaseOf = (v) => RELEASES.find((r) => r.version === v);
const isLatest = (v) => v === LATEST;

/* Other instances on the same machine: an update of one instance never replaces their helpers, removes the release
   they use, deletes their data or sweeps their credentials (SRV-16) */
const PEERS = {
  "this-mac": [{ name: "lab", version: "1.0.0-rc.4", state: "Running", note: "a test instance, not part of this organization" }],
  "studio-mac": [{ name: "sandbox", version: "1.0.0-rc.3", state: "Stopped", note: "a disposable test instance" }],
};

/* Expected time an office is away while its instance restarts on the new release */
const DOWNTIME = { HQ: "about 2 minutes", Office: "about 1 minute" };
const POLICY = "Checks for a new release every 6 hours and downloads and verifies it in the background. Installs only when someone chooses Update, or runs agentic-org upgrade on the machine.";

/* What a failed update says, and the step it stopped at */
const FAILURES = {
  linux: { step: "restart", reason: "The readiness check timed out after 90 s: the office worker didn't report ready on 1.0.0.", kept: "1.0.0-rc.4" },
};
const QUEUED = { sat: { at: "Today 07:05", by: "you" } };

/* Newest first. by: a member id, or text for the command line */
const HISTORY = [
  { at: "Today 08:40", office: "linux", from: "1.0.0-rc.4", to: "1.0.0", result: "failed", by: "maya", via: "Browser", detail: "Stopped at Restarting. Linux runner went back to 1.0.0-rc.4 and is running." },
  { at: "Today 07:05", office: "sat", from: "1.0.0-rc.2", to: "1.0.0", result: "queued", by: "you", via: "Browser", detail: "Build box is offline. It updates when it connects." },
  { at: "24 Nov 18:20", office: "hq", from: "1.0.0-rc.4", to: "1.0.0", result: "completed", by: "you", via: "Browser", detail: "Away 2 min 10 s." },
  { at: "18 Nov 10:02", office: "studio", from: "1.0.0-rc.3", to: "1.0.0-rc.4", result: "completed", by: "maya", via: "agentic-org upgrade on studio-mac", detail: "Away 48 s." },
  { at: "18 Nov 09:51", office: "linux", from: "1.0.0-rc.3", to: "1.0.0-rc.4", result: "completed", by: "you", via: "Browser", detail: "Away 1 min 05 s." },
  { at: "18 Nov 09:40", office: "hq", from: "1.0.0-rc.3", to: "1.0.0-rc.4", result: "completed", by: "you", via: "Browser", detail: "Away 1 min 52 s." },
  { at: "30 Oct 16:12", office: "sat", from: "1.0.0-rc.1", to: "1.0.0-rc.2", result: "completed", by: "maya", via: "agentic-org upgrade on mac-mini-2", detail: "Away 1 min 20 s." },
];

/* Each office's latest update log. Lines are [time, level, text]; secrets are redacted by the instance (doc-48 §5) */
const LOGS = {
  hq: [["18:18:02", "info", "upgrade requested by you (operation 0196f8a2-41c7)"], ["18:18:02", "info", "release 1.0.0 already staged in the background at 17:40"], ["18:18:03", "info", "verified manifest sha256 9f2c41e07b3d… and executable signatures"], ["18:18:04", "info", "draining: admission closed, 7 agents finishing their current step"], ["18:19:31", "info", "drained; stopping office workers and the web server"], ["18:19:40", "info", "pin changed 1.0.0-rc.4 → 1.0.0 (prior release kept)"], ["18:20:12", "info", "readiness: HQ ready, office ready, web serving"], ["18:20:12", "info", "completed in 2 min 10 s"]],
  studio: [["10:01:14", "info", "agentic-org upgrade --name default (local, maya)"], ["10:01:15", "info", "downloading 1.0.0-rc.4 for macos-arm64 (61.2 MB)"], ["10:01:39", "info", "verified manifest sha256 41d8b0c2aa19… and executable signatures"], ["10:01:40", "info", "draining: 1 agent finishing its current step"], ["10:01:52", "info", "pin changed 1.0.0-rc.3 → 1.0.0-rc.4 (prior release kept)"], ["10:02:02", "info", "readiness: office ready, connected to HQ"], ["10:02:02", "info", "completed in 48 s"]],
  linux: [["08:38:20", "info", "upgrade requested by maya (operation 0196f86c-9b02)"], ["08:38:21", "info", "downloading 1.0.0 for linux-x64 (58.7 MB)"], ["08:38:44", "info", "verified manifest sha256 9f2c41e07b3d… and executable signatures"], ["08:38:45", "info", "draining: 1 agent finishing its current step"], ["08:38:58", "info", "pin changed 1.0.0-rc.4 → 1.0.0 (prior release kept)"], ["08:39:00", "info", "starting office worker on 1.0.0"], ["08:40:30", "error", "readiness check timed out after 90 s: office worker did not report ready"], ["08:40:31", "warn", "rolling back: pin changed 1.0.0 → 1.0.0-rc.4"], ["08:40:39", "info", "office ready on 1.0.0-rc.4; agents resumed"], ["08:40:39", "error", "update failed; see agentic-org doctor --name runner. token=[redacted]"]],
  sat: [["07:05:10", "info", "upgrade requested by you (operation 0196f80f-2d6a)"], ["07:05:10", "warn", "Build box is offline (last seen 05:02); request queued at HQ"], ["07:05:10", "info", "Build box applies it when it connects: download, verify, drain, restart"]],
};

/* Session state for running updates: office -> { step, pct, opId } */
const RUNS = {};
const STEPS = [["download", "Downloading"], ["verify", "Verifying"], ["restart", "Restarting"], ["done", "Done"]];

const officeAgents = (M, k) => Object.keys(M).filter((a) => M[a].kind === "agent" && !M[a].offboarded && M[a].setup?.office === k);
const needsUpdate = (k) => !isLatest(OFFICES[k].version);
const officeState = (k) => (RUNS[k] ? "updating" : OFFICES[k].update);
const heldBack = () => Object.keys(OFFICES).filter((k) => needsUpdate(k) && (releaseOf(OFFICES[k].version)?.level ?? 0) < releaseOf(LATEST).level);

export { DOWNTIME, FAILURES, HISTORY, LATEST, LOGS, PEERS, POLICY, QUEUED, RELEASES, RUNS, STEPS, heldBack, isLatest, needsUpdate, officeAgents, officeState, releaseOf };
