// What the agent screens say and decide, as pure functions (our-org-in-the-app §5, §9; agent-run-modes-ux §0):
// words for a profile's runtime, run mode and status, and what offboarding an agent does, named before it happens.
import type { AgentProfile, AgentRuntime, Member, OrgSnapshot, RunMode, Schedule } from '$lib/proto';
import { byName, teamsOfMember } from './model';
import { managerOf } from './reporting';

/** "Claude Code", "Codex", "Antigravity", a named other as given, and plainly when it is not set. */
export function runtimeLabel(r: AgentRuntime | undefined): string {
	if (r === 'claude_code') return 'Claude Code';
	if (r === 'codex') return 'Codex';
	if (r === 'antigravity') return 'Antigravity';
	if (r && typeof r === 'object' && typeof r.other === 'string' && r.other !== '') return r.other;
	return 'Runtime not set';
}

export const RUN_MODES = [
	{ id: 'always_on', label: 'Always on', help: 'Keeps a session running and restarts it if it stops.' },
	{ id: 'on_call', label: 'On call', help: "Sleeps until it's needed: a message, a task or Run now." },
	{ id: 'scheduled', label: 'Scheduled', help: "Wakes on a schedule, and when it's needed." }
] as const;

/** What ends a run: a mirror of the defaults HQ applies to every agent (U20, ada's agent-sessions design). The profile has no fields for them, so they are shown, not edited; change them here if HQ's defaults change. */
export const RUN_LIMITS = {
	idleMinutes: 5,
	busyMinutesAtHome: 30,
	busyHoursOnATask: 2
} as const;

/** The limits in words for a mode, or '' when nothing ends a run by itself (a person starts the agent). */
export function runLimitsText(mode: string): string {
	const stop = `a run stops after ${RUN_LIMITS.busyMinutesAtHome} minutes of work in its home, or ${RUN_LIMITS.busyHoursOnATask} hours on a task.`;
	if (mode === 'on_call' || mode === 'scheduled') return `Defaults for all agents: it goes back to sleep after ${RUN_LIMITS.idleMinutes} minutes with nothing to do; ${stop}`;
	if (mode === 'always_on') return `Defaults for all agents: ${stop}`;
	return '';
}

/** The mode in words. A missing mode means a person starts the agent by hand (RUN-1); one this app can't read says so. */
export function runModeLabel(mode: RunMode | undefined): string {
	if (mode === undefined) return 'Started by a person';
	return RUN_MODES.find((m) => m.id === mode)?.label ?? 'Run mode not recognised';
}

/** "Weekdays at 9 (Europe/Lisbon)". */
export const scheduleLabel = (s: Schedule | undefined): string => (s && s.spec ? `${s.spec}${s.time_zone ? ` (${s.time_zone})` : ''}` : '');

/** Whether the agent is paused: it won't be started or woken, and messages and tasks wait (DEAL-3). */
export const isPaused = (m: Member): boolean => m.agent?.status === 'paused';

/** What the owner sees about an agent's state: a word, a few more words, and which icon goes with it. */
export interface AgentStatus {
	id: 'paused' | 'former' | 'by_person' | 'asleep' | 'unknown';
	word: string;
	detail?: string;
}

/**
 * The agent's status in words (PROF-1, RUN-2). Paused and Former come from the record. Until the office reports
 * who is working, waiting or needs the owner, an on-call agent with no live run is Asleep, one a person starts is
 * "Started by a person", and everything else says "Status unknown": "Active" is the admin flag, never a status.
 */
export function agentStatus(p: AgentProfile | undefined): AgentStatus {
	if (p?.status === 'paused') return { id: 'paused', word: 'Paused' };
	if (p?.status === 'offboarded') return { id: 'former', word: 'Former' };
	if (p?.status === 'active') {
		if (p.run_mode === undefined) return { id: 'by_person', word: 'Started by a person' };
		if (p.run_mode === 'on_call') return { id: 'asleep', word: 'Asleep', detail: 'wakes when called' };
	}
	return { id: 'unknown', word: 'Status unknown' };
}

/** One line for a quick view: "software engineer · Runs on Codex · On call". */
export function agentLine(m: Member): string {
	const a = m.agent;
	if (!a) return '';
	return [a.title || a.role, `Runs on ${runtimeLabel(a.runtime)}`, runModeLabel(a.run_mode)].join(' · ');
}

const list = (names: string[]) => (names.length <= 1 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`);

/**
 * The lines of the Offboard confirmation (§9): only what applies. The agent leaves its teams and stops leading,
 * whoever reports to it reports to its manager (or the owner), its open tasks come back unassigned (not known
 * here yet, so said in general), and its record and name stay.
 */
export function offboardSummary(s: OrgSnapshot, m: Member): string[] {
	const lines: string[] = [];
	const teams = teamsOfMember(s, m.id);
	const led = teams.filter((t) => t.lead_member_id === m.id);
	if (teams.length > 0) {
		const names = list(teams.map((t) => t.name));
		lines.push(led.length > 0 ? `${m.name} leaves ${names}, and stops leading ${list(led.map((t) => t.name))}.` : `${m.name} leaves ${names}.`);
	}
	lines.push(`Any open tasks ${m.name} has come back unassigned for you to reassign.`);
	const reports = s.members.filter((x) => !x.owner && x.reports_to_member_id === m.id).sort(byName);
	if (reports.length > 0) {
		const boss = managerOf(s, m);
		const to = boss && !boss.owner ? `${m.name}'s manager, ${boss.name}` : 'you';
		lines.push(`${list(reports.map((r) => r.name))}, who ${reports.length === 1 ? 'reports' : 'report'} to ${m.name}, will report to ${to}.`);
	}
	lines.push(`${m.name}'s record stays, and the name ${m.agent?.messaging_name ?? m.name} stays reserved: nobody else can use it.`);
	return lines;
}

/** Whether a messaging name is free: one handle namespace across agents, people and "owner" (ignoring case). */
export function messagingNameError(s: OrgSnapshot, raw: string, exceptId?: string): string | undefined {
	if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(raw)) return 'Use 1 to 64 letters, digits, - _ or ., starting with a letter or digit.';
	const same = (a: string) => a.toLowerCase() === raw.toLowerCase();
	if (same('owner')) return '"owner" is reserved for the owner. Choose another.';
	const holder = s.members.find((m) => m.id !== exceptId && !m.owner && same(m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name));
	if (holder) return holder.kind === 'agent' ? `${raw} is already ${holder.name}'s messaging name. Choose another.` : `${raw} is already ${holder.name}'s name. Choose another.`;
	return undefined;
}

/** "ac-qa-engineer-cameron" for "Cameron" with the role "QA engineer": a suggestion the owner can change. */
export function suggestMessagingName(name: string): string {
	const slug = name
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
	return slug.slice(0, 64);
}

export interface RosterRow {
	member: Member;
	teams: string[];
	/** Where it runs: the office's name, or "Unplaced". */
	office: string;
	/** Who it reports to ("You" for the owner is the owner's own name). */
	manager: string;
	/** The administrative state, for the filters. */
	status: 'active' | 'paused' | 'left' | 'unknown';
	/** What the Status column says. */
	live: AgentStatus;
}

export type RosterFilter = 'all' | 'active' | 'paused' | 'left';

/** Every agent in the organization, A–Z, with what the Agents screen lists about each (agent-first view of the chart). */
export function roster(s: OrgSnapshot, offices: { id: string; name: string }[]): RosterRow[] {
	return s.members
		.filter((m) => m.kind === 'agent')
		.sort(byName)
		.map((m) => {
			const status = m.agent?.status;
			const boss = managerOf(s, m);
			return {
				member: m,
				teams: teamsOfMember(s, m.id).map((t) => t.name),
				office: offices.find((o) => o.id === m.office_id)?.name ?? 'Unplaced',
				manager: boss?.name ?? s.members.find((x) => x.owner)?.name ?? '',
				status: status === 'active' ? 'active' : status === 'paused' ? 'paused' : status === 'offboarded' ? 'left' : 'unknown',
				live: agentStatus(m.agent)
			};
		});
}

/** The rows a filter shows. "All" leaves out agents who left: they have their own filter. */
export const filterRoster = (rows: RosterRow[], filter: RosterFilter): RosterRow[] =>
	rows.filter((r) => (filter === 'all' ? r.status !== 'left' : filter === 'left' ? r.status === 'left' : r.status === filter));
