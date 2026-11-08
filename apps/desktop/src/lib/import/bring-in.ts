// "Bring in" (our-org-in-the-app §4): turn the reviewed rows into commands on HQ. It compares with the organization as it
// is before each step and does only what is missing, so an import that was interrupted continues where it stopped (IMP-9),
// and running it again later finds nothing new (IMP-8). Nothing here touches the HR workspace or messaging.

import { addAgent, addProjectTeam, changeManager, setAgentStatus, setTeamLead, type StepResult } from '$lib/org/actions';
import { newId } from '$lib/org/commands';
import type { OrgSession } from '$lib/org/session';
import type { Member, OrgSnapshot, Team } from '$lib/proto';
import { toAppError } from '$lib/office/api';
import { isReady, OWNER, type Row } from './rows';
import { teamsToCreate, type ProjectDraft, type TeamDraft } from './teams';

export interface BringInInput {
	rows: Row[];
	drafts: TeamDraft[];
	project?: ProjectDraft;
	/** The office the agents are placed in (HQ's Mac in phase 1). */
	officeId: string;
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
	/** Everything that went wrong, each with what was being done. Empty: it all worked. */
	failures: StepResult[];
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

export async function bringIn(session: OrgSession, input: BringInInput): Promise<BringInResult> {
	const say = input.onProgress ?? (() => undefined);
	const result: BringInResult = { agents: 0, teams: 0, projects: 0, paused: 0, failures: [] };
	const ready = input.rows.filter(isReady);
	const wanted = teamsToCreate(input.rows, input.drafts);

	// 1. Teams, owners first. A team is the same team when it has the same name under the same owner.
	say('Creating teams…');
	const teamIds = new Map<string, string>();
	const order = [...wanted].sort((a, b) => depth(a, wanted) - depth(b, wanted));
	for (const draft of order) {
		const s = await session.snapshot();
		const parentId = draft.parentKey ? teamIds.get(draft.parentKey) : undefined;
		const found = s.teams.find((t) => lower(t.name) === draft.key && (t.owner_team_id ?? undefined) === parentId);
		if (found) {
			teamIds.set(draft.key, found.id);
			continue;
		}
		if (draft.parentKey && !parentId) continue; // its owner could not be made: nothing to put it under
		const id = newId();
		if (await attempt(`Create ${draft.name}`, result.failures, () => session.command({ method: 'team.create', params: { team_id: id, name: draft.name, ...(parentId ? { owner_team_id: parentId } : {}) } }))) {
			teamIds.set(draft.key, id);
			result.teams += 1;
		}
	}

	// 2. Agents, each with its own outcome. One that fails doesn't stop the others.
	say('Adding agents…');
	const memberIds = new Map<string, string>(); // row key -> member id (made now or already there)
	for (const row of input.rows) if (row.existingMemberId && row.existingKind === 'agent') memberIds.set(row.key, row.existingMemberId);
	const justMade = new Set<string>();
	for (const row of ready) {
		const there = agentByHandle(await session.snapshot(), row.messagingName);
		if (there) {
			memberIds.set(row.key, there.id); // made by an earlier try of this import
			continue;
		}
		const done = await addAgent(session, {
			name: row.name,
			messagingName: row.messagingName,
			runtime: row.runtime!,
			role: row.role!,
			title: row.title!,
			officeId: input.officeId,
			...(row.focus ? { focus: row.focus } : {}),
			...(row.model ? { model: row.model } : {}),
			...(row.home ? { home: row.home } : {}),
			teamIds: []
		});
		if (done.created.error) {
			result.failures.push(done.created);
			continue;
		}
		memberIds.set(row.key, done.memberId);
		justMade.add(row.key);
		result.agents += 1;
	}

	// 3. Paused, and the teams they go in.
	say('Putting agents in their teams…');
	for (const row of ready) {
		const memberId = memberIds.get(row.key);
		if (!memberId) continue;
		const s = await session.snapshot();
		const me = s.members.find((m) => m.id === memberId);
		if (!me) continue;
		if (row.choice === 'paused' && me.agent?.status === 'active' && justMade.has(row.key)) {
			if (await attempt(`Pause ${row.name}`, result.failures, () => setAgentStatus(session, memberId, 'paused'))) result.paused += 1;
		}
		const teamId = row.teamKey ? teamIds.get(lower(row.teamKey)) : undefined;
		if (teamId && !s.memberships.some((m) => m.team_id === teamId && m.member_id === memberId)) {
			await attempt(`Add ${row.name} to ${row.teamKey}`, result.failures, () => session.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: memberId } }));
		}
	}

	// 4. Leads and reporting lines (they need the members and memberships above).
	say('Linking leads and reporting lines…');
	for (const draft of order) {
		const teamId = teamIds.get(draft.key);
		const leadId = draft.leadKey ? memberIds.get(draft.leadKey) : undefined;
		if (!teamId || !leadId) continue;
		const team = (await session.snapshot()).teams.find((t) => t.id === teamId);
		if (team && team.lead_member_id !== leadId) await attempt(`Set the lead of ${draft.name}`, result.failures, () => setTeamLead(session, teamId, leadId));
	}
	for (const row of ready) {
		const memberId = memberIds.get(row.key);
		const managerId = row.managerKey === OWNER ? undefined : memberIds.get(row.managerKey);
		if (!memberId || !managerId) continue; // reports to the owner, which is how an agent starts
		const me = (await session.snapshot()).members.find((m) => m.id === memberId);
		if (me && me.reports_to_member_id !== managerId) await attempt(`Make ${row.name} report to their manager`, result.failures, () => changeManager(session, memberId, managerId));
	}

	// 5. The project the agents' teams share.
	if (input.project) await setUpProject(session, input.project, teamIds, result, say);
	return result;
}

/** How many owners above a team there are among the teams being made. */
function depth(draft: TeamDraft, all: TeamDraft[]): number {
	let n = 0;
	for (let t: TeamDraft | undefined = draft; t?.parentKey && n <= all.length; n++) t = all.find((x) => x.key === t!.parentKey);
	return n;
}

async function setUpProject(session: OrgSession, project: ProjectDraft, teamIds: Map<string, string>, result: BringInResult, say: (text: string) => void): Promise<void> {
	const ids = project.teamKeys.map((k) => teamIds.get(k)).filter((id): id is string => !!id);
	if (ids.length === 0) return;
	say('Setting up the project…');
	const s = await session.snapshot();
	const existing = (s.projects ?? []).find((p) => lower(p.key ?? '') === lower(project.key) || lower(p.name) === lower(project.name));
	if (existing) {
		const linked = new Set((s.project_teams ?? []).filter((l) => l.project_id === existing.id).map((l) => l.team_id));
		for (const teamId of ids.filter((id) => !linked.has(id)))
			await attempt(`Give ${existing.name} to ${teamName(s.teams, teamId)}`, result.failures, () => addProjectTeam(session, existing.id, teamId));
		return;
	}
	const validKey = /^[a-z][a-z0-9]{1,9}$/.test(lower(project.key)) && !RESERVED_KEYS.includes(lower(project.key));
	const made = await attempt(`Create ${project.name}`, result.failures, () =>
		session.command({
			method: 'project.create',
			params: {
				project_id: newId(),
				name: project.name,
				...(validKey ? { key: lower(project.key) } : {}),
				teams: ids.map((team_id) => ({ project_team_id: newId(), team_id })),
				general_area_id: newId()
			}
		})
	);
	if (made) result.projects += 1;
}

const teamName = (teams: Team[], id: string) => teams.find((t) => t.id === id)?.name ?? 'the team';
