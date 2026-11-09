// "Bring in" (our-org-in-the-app §4): turn the reviewed rows into commands on HQ. It compares with the organization as it
// is before each step and does only what is missing, so an import that was interrupted continues where it stopped (IMP-9),
// and running it again later finds nothing new (IMP-8). Nothing here touches the HR workspace or messaging.

import { addAgent, addProjectTeam, type StepResult } from '$lib/org/actions';
import { newId } from '$lib/org/commands';
import type { OrgSession } from '$lib/org/session';
import type { Member, OrgSnapshot, Team } from '$lib/proto';
import { toAppError } from '$lib/office/api';
import type { Ledger, LedgerStore } from './ledger';
import { OWNER, takesPart, type Row } from './rows';
import { teamsToCreate, type ProjectDraft, type TeamDraft } from './teams';

export interface BringInInput {
	rows: Row[];
	drafts: TeamDraft[];
	project?: ProjectDraft;
	/** The organization being filled, and the office the agents are placed in (HQ's Mac in phase 1). */
	organizationId: string;
	officeId: string;
	/** What earlier runs made. A run with none of its own finds teams and agents by name and fills in only what is empty. */
	ledger: LedgerStore;
	/** Says what it is doing, in words ("Adding agents…"). */
	onProgress?: (text: string) => void;
}

export interface BringInResult {
	/** What was newly made this time. */
	agents: number;
	teams: number;
	projects: number;
	/** Of the agents made this time, how many came in Paused. */
	paused: number;
	/** The messaging names of the agents made this time (to highlight on the chart). */
	made: string[];
	/** Everything that went wrong, each with what was being done. Empty: it all worked. */
	failures: StepResult[];
}

/** What several tries of one import did together: the counts and agents add up, and the failures are the last try's. */
export function addResults(before: BringInResult | undefined, next: BringInResult): BringInResult {
	if (!before) return next;
	return {
		agents: before.agents + next.agents,
		teams: before.teams + next.teams,
		projects: before.projects + next.projects,
		paused: before.paused + next.paused,
		made: [...before.made, ...next.made],
		failures: next.failures
	};
}

const lower = (s: string) => s.trim().toLowerCase();
/** Project keys HQ keeps for itself (org-proto `RESERVED_PROJECT_KEYS`). */
const RESERVED_KEYS = ['req', 'feat', 'adr', 'eval', 'debt', 'lim', 'exc'];

async function attempt(what: string, failures: StepResult[], run: () => Promise<unknown>): Promise<boolean> {
	try {
		await run();
		return true;
	} catch (e) {
		failures.push({ what, error: toAppError(e) });
		return false;
	}
}

const agentByHandle = (s: OrgSnapshot, handle: string): Member | undefined => s.members.find((m) => m.kind === 'agent' && m.agent && lower(m.agent.messaging_name) === lower(handle));
const isLive = (m: Member | undefined): m is Member => !!m && m.agent?.status !== 'offboarded';

/** One run at a time per organization: a second click while one is going gets that run's answer. */
const running = new Map<string, Promise<BringInResult>>();

export function bringIn(session: OrgSession, input: BringInInput): Promise<BringInResult> {
	const going = running.get(input.organizationId);
	if (going) return going;
	const run = execute(session, input).finally(() => running.delete(input.organizationId));
	running.set(input.organizationId, run);
	return run;
}

/**
 * Compare with the organization before each step and do only what is missing, and only what this import still owes: what
 * it made it links up once (its team, manager, lead) and then leaves to the owner, so a second run finds nothing new and
 * never undoes an edit (IMP-8), while a run after a failure continues where it stopped (IMP-9). Something it merely found
 * (an agent or team that was already there) gets what it is missing and nothing that it has.
 */
async function execute(session: OrgSession, input: BringInInput): Promise<BringInResult> {
	const say = input.onProgress ?? (() => undefined);
	const result: BringInResult = { agents: 0, teams: 0, projects: 0, paused: 0, made: [], failures: [] };
	const ledger = input.ledger.load();
	const keep = () => input.ledger.save(ledger);
	const taking = input.rows.filter(takesPart);
	// What this run still owes: the agents it will make, and those an earlier run made and has not finished linking. What it
	// merely finds already in the organization is the owner's: it is adopted as it is, and no team is made for it.
	const owedRows = taking.filter((r) => {
		const entry = ledger.agents[lower(r.messagingName)];
		return entry ? !entry.linked : !r.existingMemberId;
	});
	const owedTeams = input.drafts.filter((d) => ledger.teams[d.key] && !ledger.teams[d.key].led);
	const wanted = [...teamsToCreate(owedRows, input.drafts)];
	for (const d of owedTeams) if (!wanted.some((w) => w.key === d.key)) wanted.push(d);

	// 1. Teams, owners first. A team is the same team when it has the same name under the same owner.
	say('Creating teams…');
	const teamIds = new Map<string, string>();
	const order = [...wanted].sort((a, b) => depth(a, wanted) - depth(b, wanted));
	for (const draft of order) {
		const s = await session.snapshot();
		const made = ledger.teams[draft.key];
		if (made) {
			// Made by an earlier run: if the owner has since deleted it, it stays deleted.
			if (s.teams.some((t) => t.id === made.id)) teamIds.set(draft.key, made.id);
			else if (!made.led) {
				made.led = true; // nothing left to owe a team that is gone
				keep();
			}
			continue;
		}
		// An owner that isn't among the teams being made (a name the file never defined) is no owner.
		const owned = !!draft.parentKey && wanted.some((t) => t.key === draft.parentKey);
		const parentId = owned ? teamIds.get(draft.parentKey!) : undefined;
		if (owned && !parentId) {
			result.failures.push({ what: `Create ${draft.name}`, error: { source: 'app', code: 'skipped', message: `Its owner team couldn't be made, so ${draft.name} wasn't either.` } });
			continue;
		}
		const found = s.teams.find((t) => lower(t.name) === lower(draft.name) && (t.owner_team_id ?? undefined) === parentId);
		if (found) {
			// Already there (made by the owner, or by a run whose ledger is gone): adopted as it is, lead and all.
			teamIds.set(draft.key, found.id);
			ledger.teams[draft.key] = { id: found.id, led: true };
			keep();
			continue;
		}
		const id = newId();
		if (await attempt(`Create ${draft.name}`, result.failures, () => session.command({ method: 'team.create', params: { team_id: id, name: draft.name, ...(parentId ? { owner_team_id: parentId } : {}) } }))) {
			teamIds.set(draft.key, id);
			ledger.teams[draft.key] = { id, led: false };
			keep();
			result.teams += 1;
		}
	}

	// 2. Agents, each with its own outcome (one that fails doesn't stop the others). A Paused choice is made at creation.
	say('Adding agents…');
	const memberIds = new Map<string, string>(); // row key -> member id (made now or already there)
	for (const row of taking) {
		const handle = lower(row.messagingName);
		const s = await session.snapshot();
		const earlier = ledger.agents[handle];
		if (earlier) {
			// Made by an earlier run: if the owner has since removed or offboarded it, it stays that way.
			if (isLive(s.members.find((m) => m.id === earlier.id))) memberIds.set(row.key, earlier.id);
			continue;
		}
		const there = row.existingMemberId && row.existingKind === 'agent' ? s.members.find((m) => m.id === row.existingMemberId) : agentByHandle(s, row.messagingName);
		if (isLive(there)) {
			memberIds.set(row.key, there.id); // already there: the owner's, adopted as it is
			ledger.agents[handle] = { id: there.id, created: false, linked: true };
			keep();
			continue;
		}
		if (there) continue; // offboarded: its name stays reserved, and it is not part of the team
		const done = await addAgent(session, {
			name: row.name,
			messagingName: row.messagingName,
			runtime: row.runtime!,
			role: row.role!.trim(),
			title: row.title!.trim(),
			officeId: input.officeId,
			...(row.focus ? { focus: row.focus } : {}),
			...(row.model ? { model: row.model } : {}),
			...(row.home ? { home: row.home } : {}),
			status: row.choice === 'paused' ? 'paused' : 'active',
			teamIds: []
		});
		if (done.created.error) {
			result.failures.push(done.created);
			continue;
		}
		memberIds.set(row.key, done.memberId);
		ledger.agents[handle] = { id: done.memberId, created: true, linked: false };
		keep();
		result.agents += 1;
		result.made.push(row.messagingName);
		if (row.choice === 'paused') result.paused += 1;
	}

	// 3. Teams and managers of the agents this import still owes. An agent it only found keeps the teams it has.
	say('Putting agents in their teams…');
	const settled = new Map<string, boolean>(); // row key -> its team and manager are done
	const failedAdds = new Set<string>(); // `${member}:${team}` memberships that failed in this run, already reported
	for (const row of taking) {
		const memberId = memberIds.get(row.key);
		const entry = ledger.agents[lower(row.messagingName)];
		if (!memberId || !entry || entry.linked) continue;
		let ok = true;
		const teamId = row.teamKey ? teamIds.get(lower(row.teamKey)) : undefined;
		if (teamId) {
			const s = await session.snapshot();
			const mine = s.memberships.filter((m) => m.member_id === memberId);
			if (!mine.some((m) => m.team_id === teamId) && (entry.created || mine.length === 0))
				ok = await addToTeam(session, row.name, input.drafts.find((d) => d.key === lower(row.teamKey!))?.name ?? row.teamKey!, teamId, memberId, result.failures, failedAdds);
		} else if (row.teamKey && wanted.some((t) => t.key === lower(row.teamKey!)) && !ledger.teams[lower(row.teamKey)]) {
			ok = false; // its team could not be made: the failure above says so, and a retry puts it there
		}
		// A team or manager that is not there yet (its own creation failed) leaves this one unlinked for the retry.
		settled.set(row.key, ok);
	}
	say('Linking leads and reporting lines…');
	for (const row of taking) {
		const memberId = memberIds.get(row.key);
		const entry = ledger.agents[lower(row.messagingName)];
		if (!memberId || !entry || entry.linked || settled.get(row.key) === false) continue;
		const managerId = row.managerKey === OWNER ? undefined : memberIds.get(row.managerKey);
		// A manager still in the import but not complete is not there yet: the line waits for the run that has them.
		const managerStaying = row.managerKey !== OWNER && input.rows.some((r) => r.key === row.managerKey && r.choice !== 'skip' && r.existingKind !== 'person' && !r.existingOffboarded);
		let ok = true;
		if (managerId) {
			const me = (await session.snapshot()).members.find((m) => m.id === memberId);
			// Set from the version that was seen, so a change made meanwhile is a conflict, not overwritten.
			if (me && !me.reports_to_member_id) {
				ok = await attempt(`Make ${row.name} report to their manager`, result.failures, () => session.command({ method: 'member.set_reports_to', params: { member_id: memberId, expected_version: me.version, reports_to_member_id: managerId } }));
				// A refusal that a retry can't change is said once, then settled: a cycle with lines the owner already has, or
				// someone who is gone. A not_found is settled only when a fresh look confirms it: it may be a copy that lags.
				const refusal = result.failures.at(-1)?.error;
				if (!ok && refusal?.source === 'api' && refusal.code === 'cycle') ok = true;
				if (!ok && refusal?.source === 'api' && refusal.code === 'not_found') {
					const now = await session.snapshot();
					ok = !now.members.some((m) => m.id === memberId) || !now.members.some((m) => m.id === managerId);
				}
			}
		} else if (managerStaying) {
			ok = false; // its manager could not be made (yet): the line is set when a retry has them
		}
		if (ok) {
			entry.linked = true;
			keep();
		}
	}
	// Leads: a lead is a member of the team they lead (HQ refuses otherwise), and a lead the team already has stays.
	for (const draft of order) {
		const owed = ledger.teams[draft.key];
		const teamId = teamIds.get(draft.key);
		const leadRow = draft.leadKey ? taking.find((r) => r.key === draft.leadKey) : undefined;
		const leadId = leadRow ? memberIds.get(leadRow.key) : undefined;
		if (!owed || owed.led || !teamId || !leadRow || !leadId) continue;
		const s = await session.snapshot();
		const team = s.teams.find((t) => t.id === teamId);
		if (!team) continue;
		if (team.lead_member_id) {
			owed.led = true;
			keep();
			continue;
		}
		if (!s.memberships.some((m) => m.team_id === teamId && m.member_id === leadId)) {
			if (failedAdds.has(`${leadId}:${teamId}`)) continue; // step 3 failed this very add and said so
			if (!(await addToTeam(session, leadRow.name, draft.name, teamId, leadId, result.failures, failedAdds))) continue;
		}
		const fresh = (await session.snapshot()).teams.find((t) => t.id === teamId);
		if (fresh && !fresh.lead_member_id && (await attempt(`Set the lead of ${draft.name}`, result.failures, () => session.command({ method: 'team.set_lead', params: { team_id: teamId, expected_version: fresh.version, lead_member_id: leadId } })))) {
			owed.led = true;
			keep();
		}
	}

	// 5. The project the agents' teams share, once.
	if (input.project && !ledger.project) await setUpProject(session, input.project, teamIds, result, say, (id) => {
		ledger.project = { id };
		keep();
	});
	return result;
}

async function addToTeam(session: OrgSession, name: string, teamName: string, teamId: string, memberId: string, failures: StepResult[], failed: Set<string>): Promise<boolean> {
	const ok = await attempt(`Add ${name} to ${teamName}`, failures, () => session.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: memberId } }));
	if (!ok) failed.add(`${memberId}:${teamId}`);
	return ok;
}

/** How many owners above a team there are among the teams being made. */
function depth(draft: TeamDraft, all: TeamDraft[]): number {
	let n = 0;
	for (let t: TeamDraft | undefined = draft; t?.parentKey && n <= all.length; n++) t = all.find((x) => x.key === t!.parentKey);
	return n;
}

async function setUpProject(session: OrgSession, project: ProjectDraft, teamIds: Map<string, string>, result: BringInResult, say: (text: string) => void, made: (projectId: string) => void): Promise<void> {
	const ids = project.teamKeys.map((k) => teamIds.get(k)).filter((id): id is string => !!id);
	if (ids.length === 0) return;
	say('Setting up the project…');
	const s = await session.snapshot();
	const existing = (s.projects ?? []).find((p) => lower(p.key ?? '') === lower(project.key) || lower(p.name) === lower(project.name));
	if (existing) {
		const linked = new Set((s.project_teams ?? []).filter((l) => l.project_id === existing.id).map((l) => l.team_id));
		let ok = true;
		for (const teamId of ids.filter((id) => !linked.has(id)))
			ok = (await attempt(`Give ${existing.name} to ${teamName(s.teams, teamId)}`, result.failures, () => addProjectTeam(session, existing.id, teamId))) && ok;
		if (ok) made(existing.id);
		return;
	}
	const validKey = /^[a-z][a-z0-9]{1,9}$/.test(lower(project.key)) && !RESERVED_KEYS.includes(lower(project.key));
	const projectId = newId();
	const created = await attempt(`Create ${project.name}`, result.failures, () =>
		session.command({
			method: 'project.create',
			params: {
				project_id: projectId,
				name: project.name,
				...(validKey ? { key: lower(project.key) } : {}),
				teams: ids.map((team_id) => ({ project_team_id: newId(), team_id })),
				general_area_id: newId()
			}
		})
	);
	if (created) {
		result.projects += 1;
		made(projectId);
	}
}

const teamName = (teams: Team[], id: string) => teams.find((t) => t.id === id)?.name ?? 'the team';

/**
 * Whether an earlier run of this import still owes something that bringing in again would do: an agent it made whose team or
 * manager is not done, or a team whose lead is not set. (The review's own count only counts agents still to be made.)
 */
export function owesWork(ledger: Ledger, rows: Row[], drafts: TeamDraft[], now?: OrgSnapshot): boolean {
	const taking = rows.filter(takesPart);
	const waiting = (r: Row) => r.managerKey !== OWNER && rows.some((m) => m.key === r.managerKey && m.choice !== 'skip' && m.existingKind !== 'person' && !m.existingOffboarded && !takesPart(m));
	// What the engine would do: an agent or team that is gone (the owner removed or deleted it) is not owed anything.
	const liveAgent = (id: string) => !now || isLive(now.members.find((m) => m.id === id));
	const liveTeam = (id: string) => !now || now.teams.some((t) => t.id === id);
	const agents = taking.some((r) => {
		const entry = ledger.agents[lower(r.messagingName)];
		return !!entry && !entry.linked && liveAgent(entry.id) && !waiting(r);
	});
	const leads = drafts.some((d) => ledger.teams[d.key] && !ledger.teams[d.key].led && liveTeam(ledger.teams[d.key].id) && d.leadKey && taking.some((r) => r.key === d.leadKey));
	return agents || leads;
}
