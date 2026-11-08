// What the team dialogs need to know, as pure functions over a snapshot: where a team sits, the teams to
// choose from, and the name check that HQ will repeat (sibling names are unique, ignoring case).
import { validateName, trimWhiteSpace } from '$lib/name';
import type { OrgSnapshot, Team } from '$lib/proto';
import { membersOfTeam, teamTree, type TeamNode } from './model';

/** "Product › Checkout": the team's name preceded by the teams that own it. */
export function teamPath(s: OrgSnapshot, teamId: string): string {
	const names: string[] = [];
	const seen = new Set<string>();
	for (let t = s.teams.find((x) => x.id === teamId); t && !seen.has(t.id); t = t.owner_team_id ? s.teams.find((x) => x.id === t!.owner_team_id) : undefined) {
		seen.add(t.id);
		names.unshift(t.name);
	}
	return names.join(' › ');
}

export interface TeamOption {
	team: Team;
	/** How many teams own it: 0 for a top-level team. */
	depth: number;
	path: string;
}

/** Every team in tree order (each followed by the teams it owns), skipping `exclude` and everything under it. */
export function teamOptions(s: OrgSnapshot, exclude: ReadonlySet<string> = new Set()): TeamOption[] {
	const out: TeamOption[] = [];
	const walk = (nodes: TeamNode[], depth: number) => {
		for (const n of nodes) {
			if (exclude.has(n.team.id)) continue;
			out.push({ team: n.team, depth, path: teamPath(s, n.team.id) });
			walk(n.children, depth + 1);
		}
	};
	walk(teamTree(s), 0);
	return out;
}

export type TeamNameCheck = { ok: true; name: string } | { ok: false; message: string };

const same = (a: string, b: string) => trimWhiteSpace(a).toLowerCase() === trimWhiteSpace(b).toLowerCase();

/**
 * A team's name before sending (TEAM-5): the name rule, then "unique among the teams with the same owner,
 * ignoring case" (top-level teams are siblings of each other). `exceptId` is the team being renamed.
 */
export function checkTeamName(s: OrgSnapshot, raw: string, ownerTeamId: string | undefined, exceptId?: string): TeamNameCheck {
	const checked = validateName(raw, 'Name a team (1–64 characters).');
	if (!checked.ok) return checked;
	const clash = s.teams.some((t) => t.id !== exceptId && (t.owner_team_id ?? undefined) === (ownerTeamId ?? undefined) && same(t.name, checked.name));
	if (clash) {
		const owner = ownerTeamId ? s.teams.find((t) => t.id === ownerTeamId)?.name : undefined;
		return { ok: false, message: owner ? `${owner} already has a team called ${checked.name}.` : `There's already a top-level team called ${checked.name}.` };
	}
	return checked;
}

/**
 * The lines of the delete confirmation (TEAM-9, spec §7.10): what happens to the members and projects, naming
 * only what applies. Refused up front for a team that owns teams.
 */
export function deleteSummary(s: OrgSnapshot, team: Team): { refused: string | undefined; lines: string[] } {
	const owns = s.teams.filter((t) => t.owner_team_id === team.id);
	if (owns.length > 0)
		return {
			refused: `${team.name} still has ${owns.length} ${owns.length === 1 ? 'team' : 'teams'} under it (${owns.map((t) => t.name).join(', ')}). Move or delete ${owns.length === 1 ? 'it' : 'them'} first.`,
			lines: []
		};
	const members = membersOfTeam(s, team.id);
	const elsewhere = new Set(s.memberships.filter((m) => m.team_id !== team.id).map((m) => m.member_id));
	const alone = members.filter((m) => !elsewhere.has(m.id));
	const lines: string[] = [];
	if (members.length === 0) lines.push(`No one is in ${team.name}.`);
	else {
		const stay = members.filter((m) => elsewhere.has(m.id));
		if (stay.length > 0) lines.push(`${list(stay.map((m) => m.name))} stay${stay.length === 1 ? 's' : ''} in the organization and in their other teams.`);
		if (alone.length > 0) lines.push(`${list(alone.map((m) => m.name))} ${alone.length === 1 ? 'moves' : 'move'} to Not in a team.`);
	}
	const links = s.project_teams ?? [];
	const linked = new Set(links.filter((p) => p.team_id === team.id).map((p) => p.project_id));
	const kept = new Set(links.filter((p) => p.team_id !== team.id).map((p) => p.project_id));
	for (const project of (s.projects ?? []).filter((p) => linked.has(p.id))) {
		if (kept.has(project.id)) {
			const others = links.filter((p) => p.project_id === project.id && p.team_id !== team.id).map((p) => s.teams.find((t) => t.id === p.team_id)?.name ?? '');
			lines.push(`${project.name} stays with ${list(others)}.`);
		} else lines.push(`${project.name} has no other team, so it moves to Unassigned projects.`);
	}
	return { refused: undefined, lines };
}

/** "A", "A and B", "A, B and C". */
const list = (names: string[]) => (names.length <= 1 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`);
