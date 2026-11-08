// What the org chart's dialogs do, as functions over `OrgApi`: each is a small sequence of commands, and each
// reports every failure instead of stopping at the first (TEAM-5, TEAM-6, TEAM-7). Pure of the UI.
import { toAppError } from '$lib/office/api';
import type { AppError, OrgSnapshot, Team } from '$lib/proto';
import type { OrgApi } from './api';
import { newId } from './commands';
import { membersOfTeam } from './model';

export interface StepResult {
	/** What was being done, in words (for "Couldn't add Noor to Web: …"). */
	what: string;
	error?: AppError;
}

async function attempt(what: string, run: () => Promise<unknown>): Promise<StepResult> {
	try {
		await run();
		return { what };
	} catch (e) {
		return { what, error: toAppError(e) };
	}
}

export interface NewTeamInput {
	name: string;
	ownerTeamId?: string;
	memberIds: string[];
	leadId?: string;
}

export interface NewTeamResult {
	teamId: string;
	/** The team itself: when this fails nothing else was tried and the form stays open. */
	created: StepResult;
	/** Members and lead, each with its own outcome. */
	steps: StepResult[];
}

/** Create a team, add its members, then set its lead (who must be one of them). Never throws. */
export async function createTeam(api: OrgApi, input: NewTeamInput): Promise<NewTeamResult> {
	const teamId = newId();
	const created = await attempt(`Create ${input.name}`, () =>
		api.command({ method: 'team.create', params: { team_id: teamId, name: input.name, owner_team_id: input.ownerTeamId } })
	);
	if (created.error) return { teamId, created, steps: [] };

	const steps: StepResult[] = [];
	const added = new Set<string>();
	for (const memberId of input.memberIds) {
		const step = await attempt(`Add ${memberId}`, () =>
			api.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: memberId } })
		);
		steps.push(step);
		if (!step.error) added.add(memberId);
	}
	// The lead comes from the members that were actually added.
	if (input.leadId && added.has(input.leadId)) {
		const team = (await api.snapshot()).teams.find((t) => t.id === teamId);
		if (team) steps.push(await attempt('Set the lead', () => api.command({ method: 'team.set_lead', params: { team_id: teamId, expected_version: team.version, lead_member_id: input.leadId } })));
	}
	return { teamId, created, steps };
}

/**
 * Make a team's members exactly `desired`: add who is missing, remove who is no longer wanted. Each change
 * is its own command, so one failure doesn't lose the others (TEAM-6). Says whether a removal clears the lead.
 */
export async function setTeamMembers(api: OrgApi, teamId: string, desired: string[]): Promise<{ steps: StepResult[] }> {
	const s = await api.snapshot();
	const have = new Map(s.memberships.filter((m) => m.team_id === teamId).map((m) => [m.member_id, m]));
	const steps: StepResult[] = [];
	for (const id of desired.filter((d) => !have.has(d)))
		steps.push(await attempt(`Add ${id}`, () => api.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: id } })));
	for (const [id, ms] of have)
		if (!desired.includes(id))
			steps.push(await attempt(`Remove ${id}`, () => api.command({ method: 'team.remove_member', params: { membership_id: ms.id, expected_version: ms.version } })));
	return { steps };
}

/** Whether removing these members would clear the team's lead (to warn before doing it). */
export function wouldClearLead(s: OrgSnapshot, team: Team, desired: string[]): boolean {
	return !!team.lead_member_id && !desired.includes(team.lead_member_id) && membersOfTeam(s, team.id).some((m) => m.id === team.lead_member_id);
}

/** Move a team under another (or to the top). Returns what is needed to undo it (TEAM-7, 8 s). */
export async function moveTeam(api: OrgApi, teamId: string, toTeamId: string | undefined): Promise<{ undo: () => Promise<void> }> {
	const before = (await api.snapshot()).teams.find((t) => t.id === teamId);
	if (!before) throw { source: 'api', code: 'not_found', message: "That team isn't in the organization any more." } satisfies AppError;
	await api.command({ method: 'team.move', params: { team_id: teamId, expected_version: before.version, owner_team_id: toTeamId } });
	return {
		undo: async () => {
			const now = (await api.snapshot()).teams.find((t) => t.id === teamId);
			if (now) await api.command({ method: 'team.move', params: { team_id: teamId, expected_version: now.version, owner_team_id: before.owner_team_id } });
		}
	};
}

/** What deleting a team does, for the confirmation (TEAM-9): refused if it owns teams; else who and what moves. */
export function deleteConsequences(s: OrgSnapshot, team: Team) {
	const owns = s.teams.filter((t) => t.owner_team_id === team.id);
	const members = membersOfTeam(s, team.id);
	const alsoElsewhere = new Set(s.memberships.filter((m) => m.team_id !== team.id).map((m) => m.member_id));
	const toNotInATeam = members.filter((m) => !alsoElsewhere.has(m.id));
	const projectIds = new Set((s.project_teams ?? []).filter((p) => p.team_id === team.id).map((p) => p.project_id));
	const others = new Set((s.project_teams ?? []).filter((p) => p.team_id !== team.id).map((p) => p.project_id));
	const unassigned = (s.projects ?? []).filter((p) => projectIds.has(p.id) && !others.has(p.id));
	return { refused: owns.length > 0, owns, toNotInATeam, unassigned };
}

/** Delete a team. HQ refuses one that owns teams. */
export const deleteTeam = (api: OrgApi, team: Team) => api.command({ method: 'team.delete', params: { team_id: team.id, expected_version: team.version } });

/** The teams a team could be moved under: never itself or a team below it (TEAM-7). */
export function moveTargets(s: OrgSnapshot, team: Team): Team[] {
	const below = new Set<string>();
	const walk = (id: string) => s.teams.filter((t) => t.owner_team_id === id).forEach((t) => !below.has(t.id) && (below.add(t.id), walk(t.id)));
	walk(team.id);
	return s.teams.filter((t) => t.id !== team.id && !below.has(t.id) && t.id !== team.owner_team_id).sort((a, b) => a.name.localeCompare(b.name));
}
