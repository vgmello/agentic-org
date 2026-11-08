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

/** The mode in words. A missing mode means a person starts the agent by hand (RUN-1); one this app can't read says so. */
export function runModeLabel(mode: RunMode | undefined): string {
	if (mode === undefined) return 'Started by a person';
	return RUN_MODES.find((m) => m.id === mode)?.label ?? 'Run mode not recognised';
}

/** "Weekdays at 9 (Europe/Lisbon)". */
export const scheduleLabel = (s: Schedule | undefined): string => (s && s.spec ? `${s.spec}${s.time_zone ? ` (${s.time_zone})` : ''}` : '');

/** Whether the agent is paused: it won't be started or woken, and messages and tasks wait (DEAL-3). */
export const isPaused = (m: Member): boolean => m.agent?.status === 'paused';

/** The words of an agent's status. Unknown is never read as working. */
export function statusWord(p: AgentProfile | undefined): string {
	switch (p?.status) {
		case 'active':
			return 'Active';
		case 'paused':
			return 'Paused';
		case 'offboarded':
			return 'Left';
		default:
			return 'Status unknown';
	}
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
