// The org chart's rules as pure functions over an `OrgSnapshot`, so every view draws from one
// place and the rules are tested without a browser. Nothing here changes a snapshot.
import type { Member, OrgSnapshot, ProjectSummary, Team } from '$lib/proto';

export const byName = <T extends { name: string }>(a: T, b: T) =>
	a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);

export const owner = (s: OrgSnapshot): Member | undefined => s.members.find((m) => m.owner);

/** Everyone but the owner: the owner is shown once, apart, and never counted on a team (OWNER-1). */
export const nonOwners = (s: OrgSnapshot): Member[] => s.members.filter((m) => !m.owner);

/** An agent that has left the organization: its record and handle stay, but it takes no part in the chart. */
export const isFormer = (m: Member): boolean => m.kind === 'agent' && m.agent?.status === 'offboarded';

/**
 * The members a search matches (TEAM-11): by name, and for an agent by messaging name, role and title, ignoring case
 * and accents. `undefined` when nothing is typed (so nothing is dimmed).
 */
export function searchMembers(s: OrgSnapshot, query: string): Set<string> | undefined {
	const fold = (x: string) => x.normalize('NFKD').replace(/[\u0300-\u036f]/g, '').toLowerCase();
	const q = fold(query.trim());
	if (q === '') return undefined;
	const hits = new Set<string>();
	for (const m of nonOwners(s)) {
		const fields = [m.name, m.contact ?? '', m.agent?.messaging_name ?? '', m.agent?.role ?? '', m.agent?.title ?? ''];
		if (fields.some((f) => fold(f).includes(q))) hits.add(m.id);
	}
	return hits;
}

/** The organization as the chart shows it: without the agents who have left (they are listed apart). */
export const withoutFormer = (s: OrgSnapshot): OrgSnapshot => (s.members.some(isFormer) ? { ...s, members: s.members.filter((m) => !isFormer(m)) } : s);

/** The organization without its paused agents (DEAL-5, "Show paused" off): they stay members, just not drawn. */
export const withoutPaused = (s: OrgSnapshot): OrgSnapshot =>
	s.members.some((m) => m.agent?.status === 'paused') ? { ...s, members: s.members.filter((m) => m.agent?.status !== 'paused') } : s;

/** The agents who have left, most recent first (HQ stamps when). */
export const formerMembers = (s: OrgSnapshot): Member[] =>
	s.members.filter(isFormer).sort((a, b) => (b.agent?.offboarded_at ?? '').localeCompare(a.agent?.offboarded_at ?? '') || byName(a, b));

export const memberById = (s: OrgSnapshot, id: string | undefined): Member | undefined =>
	id === undefined ? undefined : s.members.find((m) => m.id === id);

export interface TeamNode {
	team: Team;
	children: TeamNode[];
}

/**
 * The ownership tree: top-level teams first, A–Z, each with its sub-teams. A team whose owner
 * isn't in the snapshot is treated as top level, and a loop can't hang the tree or hide a team (every
 * team appears exactly once).
 */
export function teamTree(s: OrgSnapshot): TeamNode[] {
	const ids = new Set(s.teams.map((t) => t.id));
	const seen = new Set<string>();
	const children = new Map<string | undefined, Team[]>();
	for (const t of s.teams) {
		const parent = t.owner_team_id && ids.has(t.owner_team_id) && t.owner_team_id !== t.id ? t.owner_team_id : undefined;
		children.set(parent, [...(children.get(parent) ?? []), t]);
	}
	const build = (t: Team): TeamNode => {
		seen.add(t.id);
		return {
			team: t,
			children: (children.get(t.id) ?? []).filter((c) => !seen.has(c.id)).sort(byName).map(build)
		};
	};
	const roots = (children.get(undefined) ?? []).sort(byName).map(build);
	// Teams that own each other in a loop have no way in from the top: draw each as its own root (lowest
	// name first), so no team, and no team below one, disappears. (The contract refuses loops, so this only
	// protects against corrupt data; a team hanging off a loop may be drawn as a root rather than under it.)
	for (const t of [...s.teams].sort(byName)) if (!seen.has(t.id)) roots.push(build(t));
	return roots.sort((a, b) => byName(a.team, b.team));
}

/** Members of a team, A–Z. The owner is never one (OWNER-1). */
export function membersOfTeam(s: OrgSnapshot, teamId: string): Member[] {
	const ids = new Set(s.memberships.filter((m) => m.team_id === teamId).map((m) => m.member_id));
	return nonOwners(s).filter((m) => ids.has(m.id)).sort(byName);
}

/** How a member's kind reads in words, for screens and screen readers. */
export const kindWord = (kind: Member['kind']): 'Agent' | 'Person' | 'Member' => (kind === 'agent' ? 'Agent' : kind === 'person' ? 'Person' : 'Member');

export const agentsOf = (members: Member[]) => members.filter((m) => m.kind === 'agent');
export const peopleOf = (members: Member[]) => members.filter((m) => m.kind === 'person');
/** Members of a kind this build doesn't know (the contract may grow one): shown, never dropped. */
export const othersOf = (members: Member[]) => members.filter((m) => m.kind !== 'agent' && m.kind !== 'person');

/** The teams a member is in, A–Z (TEAM-2). */
export function teamsOfMember(s: OrgSnapshot, memberId: string): Team[] {
	const ids = new Set(s.memberships.filter((m) => m.member_id === memberId).map((m) => m.team_id));
	return s.teams.filter((t) => ids.has(t.id)).sort(byName);
}

/** The other teams a member is in, shown as the "+N teams" badge on one team's card (TEAM-2). */
export const otherTeamsOf = (s: OrgSnapshot, memberId: string, teamId: string): Team[] =>
	teamsOfMember(s, memberId).filter((t) => t.id !== teamId);

/** Agents and people in no team, A–Z; the owner never (TEAM-3). */
export function notInATeam(s: OrgSnapshot): Member[] {
	const teamIds = new Set(s.teams.map((t) => t.id));
	const inATeam = new Set(s.memberships.filter((m) => teamIds.has(m.team_id)).map((m) => m.member_id));
	return nonOwners(s).filter((m) => !inATeam.has(m.id)).sort(byName);
}

/** A team's lead: a member of the team, never the owner (the contract guarantees both; the chart doesn't depend on it). */
export function leadOf(s: OrgSnapshot, team: Team): Member | undefined {
	const lead = memberById(s, team.lead_member_id);
	return lead && !lead.owner && membersOfTeam(s, team.id).some((m) => m.id === lead.id) ? lead : undefined;
}

/** Direct sub-teams of a team. */
export const subTeamCount = (node: TeamNode) => node.children.length;

// ---- projects ---------------------------------------------------------------------------------

/** The teams working on a project, A–Z. */
export function teamsOfProject(s: OrgSnapshot, projectId: string): Team[] {
	const ids = new Set((s.project_teams ?? []).filter((p) => p.project_id === projectId).map((p) => p.team_id));
	return s.teams.filter((t) => ids.has(t.id)).sort(byName);
}

/** The projects a team works on, A–Z (TEAM-1). */
export function projectsOfTeam(s: OrgSnapshot, teamId: string): ProjectSummary[] {
	const ids = new Set((s.project_teams ?? []).filter((p) => p.team_id === teamId).map((p) => p.project_id));
	return (s.projects ?? []).filter((p) => ids.has(p.id)).sort(byName);
}

/** The other teams sharing a project with `teamId`; empty when it isn't shared (TEAM-13). */
export const otherTeamsOnProject = (s: OrgSnapshot, projectId: string, teamId: string): Team[] =>
	teamsOfProject(s, projectId).filter((t) => t.id !== teamId);

/** Projects with no team, A–Z (TEAM-14). */
export function unassignedProjects(s: OrgSnapshot): ProjectSummary[] {
	const teamIds = new Set(s.teams.map((t) => t.id));
	const assigned = new Set((s.project_teams ?? []).filter((p) => teamIds.has(p.team_id)).map((p) => p.project_id));
	return (s.projects ?? []).filter((p) => !assigned.has(p.id)).sort(byName);
}

// ---- office filter ----------------------------------------------------------------------------

export type OfficeFilter = { kind: 'all' } | { kind: 'office'; id: string } | { kind: 'unplaced' };

/**
 * Whether a member is highlighted by the office filter (TEAM-4, PEOPLE-6). `undefined` means no
 * filter is active. Only agents are placed in offices, so people are never highlighted. `knownOffices`, when
 * given, makes an agent whose office isn't among them count as unplaced.
 */
export function highlightedByOffice(member: Member, filter: OfficeFilter, knownOffices?: ReadonlySet<string>): boolean | undefined {
	if (filter.kind === 'all') return undefined;
	if (member.kind !== 'agent') return false;
	// An agent placed in an office that isn't in the list (it was removed) is unplaced too.
	const placed = !!member.office_id && (!knownOffices || knownOffices.has(member.office_id));
	return filter.kind === 'unplaced' ? !placed : member.office_id === filter.id;
}

// ---- counts -----------------------------------------------------------------------------------

/** Agents and people in the organization, the owner excluded (OWNER-1). */
export function organizationCounts(s: OrgSnapshot) {
	const members = nonOwners(s);
	return { agents: agentsOf(members).length, people: peopleOf(members).length, others: othersOf(members).length, teams: s.teams.length };
}

/**
 * Normalise what an office sent: a member kind this build doesn't know becomes `unknown`, so the rest of
 * the app only ever sees the kinds it handles (like `toAppError` does for error codes).
 */
export function normalizeKinds(s: OrgSnapshot): OrgSnapshot {
	return { ...s, members: s.members.map(normalizeMember) };
}

const RUNTIMES = new Set(['claude_code', 'codex', 'antigravity', 'unknown']);
const STATUSES = new Set(['active', 'paused', 'offboarded']);
const RUN_MODES = new Set(['always_on', 'on_call', 'scheduled']);

/** One member as the app reads it: an unknown kind, and an agent's unknown runtime, status or run mode, become `unknown`. */
function normalizeMember(m: Member): Member {
	const kind = m.kind === 'agent' || m.kind === 'person' ? m.kind : ('unknown' as const);
	if (!m.agent) return kind === m.kind ? m : { ...m, kind };
	const a = m.agent;
	const runtime = typeof a.runtime === 'object' && a.runtime !== null && typeof (a.runtime as { other?: unknown }).other === 'string' ? a.runtime : RUNTIMES.has(a.runtime as string) ? a.runtime : ('unknown' as const);
	const status = STATUSES.has(a.status) ? a.status : ('unknown' as const);
	const run_mode = a.run_mode === undefined ? undefined : RUN_MODES.has(a.run_mode) ? a.run_mode : ('unknown' as const);
	return { ...m, kind, agent: { ...a, runtime, status, ...(run_mode === undefined ? {} : { run_mode }) } };
}
