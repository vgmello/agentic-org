// What the Projects screens show, from what the contract has today: projects, the teams that work on each, and
// those teams' leads (the team's lead also leads its projects). Tasks, bugs and docs come with the work-items contract.
import type { Member, OrgSnapshot, ProjectSummary, Team } from '$lib/proto';
import { byName, leadOf } from './model';

export interface ProjectRow {
	project: ProjectSummary;
	/** The teams working on it, A–Z. */
	teams: Team[];
	/** Each team's lead, where it has one (a shared project shows one per team). */
	leads: { team: Team; lead: Member }[];
	/** Worked on by several teams: shown with a Shared badge. */
	shared: boolean;
	/** No team yet: needs Assign team… */
	unassigned: boolean;
}

export function projectRows(s: OrgSnapshot): ProjectRow[] {
	return (s.projects ?? [])
		.map((project): ProjectRow => {
			const teamIds = new Set((s.project_teams ?? []).filter((l) => l.project_id === project.id).map((l) => l.team_id));
			const teams = s.teams.filter((t) => teamIds.has(t.id)).sort(byName);
			const leads = teams.flatMap((team) => {
				const lead = leadOf(s, team);
				return lead ? [{ team, lead }] : [];
			});
			return { project, teams, leads, shared: teams.length > 1, unassigned: teams.length === 0 };
		})
		.sort((a, b) => a.project.name.localeCompare(b.project.name, undefined, { sensitivity: 'base' }));
}

/** "Leads: Riley (Checkout), ada (Platform)", "Lead: Riley", or "No lead" (spec §3, TEAM-15). Unassigned: nothing. */
export function leadsLine(row: ProjectRow): string {
	if (row.unassigned) return '';
	if (row.leads.length === 0) return 'No lead';
	if (row.leads.length === 1 && row.teams.length === 1) return `Lead: ${row.leads[0].lead.name}`;
	return `${row.leads.length === 1 ? 'Lead' : 'Leads'}: ${row.leads.map((l) => `${l.lead.name} (${l.team.name})`).join(', ')}`;
}

export const projectById = (s: OrgSnapshot, id: string): ProjectRow | undefined => projectRows(s).find((r) => r.project.id === id);
