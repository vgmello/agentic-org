// What the org chart's dialogs do, as functions over `OrgApi`: each is a small sequence of commands, and each
// reports every failure instead of stopping at the first (TEAM-5, TEAM-6, TEAM-7). Pure of the UI.
import { toAppError } from '$lib/office/api';
import type { AppError, OrgSnapshot, Team } from '$lib/proto';
import { newId } from './commands';
import { membersOfTeam } from './model';
import type { OrgSession } from './session';

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
export async function createTeam(api: OrgSession, input: NewTeamInput, nameOf: (id: string) => string = (id) => id): Promise<NewTeamResult> {
	const teamId = newId();
	const created = await attempt(`Create ${input.name}`, () =>
		api.command({ method: 'team.create', params: { team_id: teamId, name: input.name, owner_team_id: input.ownerTeamId } })
	);
	if (created.error) return { teamId, created, steps: [] };

	const steps: StepResult[] = [];
	const added = new Set<string>();
	for (const memberId of input.memberIds) {
		const step = await attempt(`Add ${nameOf(memberId)} to ${input.name}`, () =>
			api.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: memberId } })
		);
		steps.push(step);
		if (!step.error) added.add(memberId);
	}
	// The lead comes from the members that were actually added.
	if (input.leadId && added.has(input.leadId)) {
		const team = (await api.snapshot()).teams.find((t) => t.id === teamId);
		if (team) steps.push(await attempt(`Set ${nameOf(input.leadId)} as the lead`, () => api.command({ method: 'team.set_lead', params: { team_id: teamId, expected_version: team.version, lead_member_id: input.leadId } })));
	}
	return { teamId, created, steps };
}

/**
 * Make a team's members exactly `desired`: add who is missing, remove who is no longer wanted. Each change
 * is its own command, so one failure doesn't lose the others (TEAM-6). Says whether a removal clears the lead.
 */
export async function setTeamMembers(api: OrgSession, teamId: string, desired: string[], nameOf: (id: string) => string = (id) => id): Promise<{ steps: StepResult[] }> {
	const s = await api.snapshot();
	const have = new Map(s.memberships.filter((m) => m.team_id === teamId).map((m) => [m.member_id, m]));
	const steps: StepResult[] = [];
	for (const id of desired.filter((d) => !have.has(d)))
		steps.push(await attempt(`Add ${nameOf(id)}`, () => api.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: id } })));
	for (const [id, ms] of have)
		if (!desired.includes(id))
			steps.push(await attempt(`Remove ${nameOf(id)}`, () => api.command({ method: 'team.remove_member', params: { membership_id: ms.id, expected_version: ms.version } })));
	return { steps };
}

/** Set a team's lead (or clear it) against the team's current version. */
export async function setTeamLead(api: OrgSession, teamId: string, leadId: string | undefined): Promise<void> {
	const team = (await api.snapshot()).teams.find((t) => t.id === teamId);
	if (!team) throw { source: 'api', code: 'not_found', message: "That team isn't in the organization any more." } satisfies AppError;
	await api.command({ method: 'team.set_lead', params: { team_id: teamId, expected_version: team.version, lead_member_id: leadId } });
}

/** Whether removing these members would clear the team's lead (to warn before doing it). */
export function wouldClearLead(s: OrgSnapshot, team: Team, desired: string[]): boolean {
	return !!team.lead_member_id && !desired.includes(team.lead_member_id) && membersOfTeam(s, team.id).some((m) => m.id === team.lead_member_id);
}

/** Move a team under another (or to the top). Returns what is needed to undo it (TEAM-7, 8 s). */
export async function moveTeam(api: OrgSession, teamId: string, toTeamId: string | undefined): Promise<{ undo: () => Promise<void> }> {
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
export const deleteTeam = (api: OrgSession, team: Team) => api.command({ method: 'team.delete', params: { team_id: team.id, expected_version: team.version } });

/** The teams a team could be moved under: never itself or a team below it (TEAM-7). */
export function moveTargets(s: OrgSnapshot, team: Team): Team[] {
	const below = new Set<string>();
	const walk = (id: string) => s.teams.filter((t) => t.owner_team_id === id).forEach((t) => !below.has(t.id) && (below.add(t.id), walk(t.id)));
	walk(team.id);
	return s.teams.filter((t) => t.id !== team.id && !below.has(t.id) && t.id !== team.owner_team_id).sort((a, b) => a.name.localeCompare(b.name));
}

/** Give a project to a team (TEAM-8). HQ refuses a team that already works on it. */
export const addProjectTeam = (api: OrgSession, projectId: string, teamId: string) =>
	api.command({ method: 'project.add_team', params: { project_team_id: newId(), project_id: projectId, team_id: teamId } });

/** Take a team off a project, against the link's current version. A project left with no team is unassigned. */
export async function removeProjectTeam(api: OrgSession, projectId: string, teamId: string): Promise<void> {
	const link = ((await api.snapshot()).project_teams ?? []).find((l) => l.project_id === projectId && l.team_id === teamId);
	if (!link) throw { source: 'api', code: 'not_found', message: "That team isn't on the project any more." } satisfies AppError;
	await api.command({ method: 'project.remove_team', params: { project_team_id: link.id, expected_version: link.version } });
}

export interface NewPersonInput {
	name: string;
	contact?: string;
	/** Teams to put them in (optional). */
	teamIds: string[];
	/** Who they report to; none means the owner. */
	managerId?: string;
}

export interface NewPersonResult {
	memberId: string;
	/** The person themselves: when this fails nothing else was tried and the form stays open. */
	created: StepResult;
	steps: StepResult[];
}

/** Add a person (PEOPLE-3), then their teams and manager, each its own command. Never throws. */
export async function addPerson(api: OrgSession, input: NewPersonInput, teamName: (id: string) => string = (id) => id): Promise<NewPersonResult> {
	const memberId = newId();
	const created = await attempt(`Add ${input.name}`, () =>
		api.command({ method: 'member.add_person', params: { member_id: memberId, name: input.name, contact: input.contact || undefined } })
	);
	if (created.error) return { memberId, created, steps: [] };
	const steps: StepResult[] = [];
	for (const teamId of input.teamIds)
		steps.push(await attempt(`Add ${input.name} to ${teamName(teamId)}`, () => api.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: memberId } })));
	if (input.managerId) {
		const me = (await api.snapshot()).members.find((m) => m.id === memberId);
		if (me) steps.push(await attempt('Set their manager', () => api.command({ method: 'member.set_reports_to', params: { member_id: memberId, expected_version: me.version, reports_to_member_id: input.managerId } })));
	}
	return { memberId, created, steps };
}

/** Rename a person and/or change their contact (an empty contact clears it), against their current version. */
export async function updatePerson(api: OrgSession, memberId: string, change: { name?: string; contact?: string }): Promise<void> {
	const m = (await api.snapshot()).members.find((x) => x.id === memberId);
	if (!m) throw { source: 'api', code: 'not_found', message: "That person isn't in the organization any more." } satisfies AppError;
	const clear = change.contact !== undefined && change.contact.trim() === '';
	await api.command({
		method: 'member.update_person',
		params: { member_id: memberId, expected_version: m.version, ...(change.name !== undefined ? { name: change.name } : {}), ...(change.contact !== undefined && !clear ? { contact: change.contact.trim() } : {}), ...(clear ? { clear_contact: true } : {}) }
	});
}

/** Remove someone from the organization (never the owner; an agent leaves through the HR flow). */
export async function removePerson(api: OrgSession, memberId: string): Promise<void> {
	const m = (await api.snapshot()).members.find((x) => x.id === memberId);
	if (!m) throw { source: 'api', code: 'not_found', message: "That person isn't in the organization any more." } satisfies AppError;
	await api.command({ method: 'member.remove', params: { member_id: memberId, expected_version: m.version } });
}

/** Change who a member reports to (REPORT-3/4): none means the owner. Returns how to undo it (8 s). */
export async function changeManager(api: OrgSession, memberId: string, managerId: string | undefined): Promise<{ undo: () => Promise<void> }> {
	const before = (await api.snapshot()).members.find((m) => m.id === memberId);
	if (!before) throw { source: 'api', code: 'not_found', message: "That person isn't in the organization any more." } satisfies AppError;
	await api.command({ method: 'member.set_reports_to', params: { member_id: memberId, expected_version: before.version, reports_to_member_id: managerId } });
	return {
		undo: async () => {
			const now = (await api.snapshot()).members.find((m) => m.id === memberId);
			if (!now) return;
			await api.command({ method: 'member.set_reports_to', params: { member_id: memberId, expected_version: now.version, reports_to_member_id: before.reports_to_member_id } });
		}
	};
}

/** The teams a member is in with their titles, to compare with what the dialog asks for. */
export function teamTitlesOf(s: OrgSnapshot, memberId: string): Map<string, { membershipId: string; version: number; title?: string }> {
	return new Map(s.memberships.filter((m) => m.member_id === memberId).map((m) => [m.team_id, { membershipId: m.id, version: m.version, title: m.title_in_team }]));
}

/** Teams whose lead is this member and would lose them if they left `desired` (TEAM-6): to warn first. */
export function ledTeamsLeft(s: OrgSnapshot, memberId: string, desired: ReadonlySet<string>): Team[] {
	return s.teams.filter((t) => t.lead_member_id === memberId && !desired.has(t.id));
}

/**
 * Make a member's teams exactly `desired` (team id → title): add where missing, remove where unchecked, change a
 * title. One command per change, so one failure doesn't lose the rest (TEAM-6). `teamName` words each step.
 */
export async function setMemberTeams(api: OrgSession, memberId: string, desired: Map<string, string | undefined>, teamName: (id: string) => string = (id) => id): Promise<{ steps: StepResult[] }> {
	const have = teamTitlesOf(await api.snapshot(), memberId);
	const steps: StepResult[] = [];
	for (const [teamId, title] of desired) {
		const mine = have.get(teamId);
		const wanted = title?.trim() || undefined;
		if (!mine)
			steps.push(await attempt(`Add to ${teamName(teamId)}`, () => api.command({ method: 'team.add_member', params: { membership_id: newId(), team_id: teamId, member_id: memberId, title_in_team: wanted } })));
		else if ((mine.title ?? undefined) !== wanted)
			steps.push(await attempt(`Change the title in ${teamName(teamId)}`, () => api.command({ method: 'team.set_title', params: { membership_id: mine.membershipId, expected_version: mine.version, title_in_team: wanted } })));
	}
	for (const [teamId, mine] of have)
		if (!desired.has(teamId))
			steps.push(await attempt(`Remove from ${teamName(teamId)}`, () => api.command({ method: 'team.remove_member', params: { membership_id: mine.membershipId, expected_version: mine.version } })));
	return { steps };
}
