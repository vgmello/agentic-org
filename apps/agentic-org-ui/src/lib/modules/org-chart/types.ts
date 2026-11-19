// What the Org chart reads (offices-and-teams-ux §7.1, §7.8). Field names follow the employee contract
// (docs/design/organization-data/04-agent-structure.md): `reportsToId` is the explicit `reports_to_employee_id`,
// `homeTeam` the required `home_team_id`, `revision` the employee record's revision a change is checked against.
// Ownership is the organization's, never an employee field: `ownerId` names the person who holds it.
import type { EmployeeStatus, Placement, TeamRef } from '$lib/modules/members';

export type { TeamRef } from '$lib/modules/members';

/** One active employee: a node of the reporting tree. */
export interface ChartEmployee {
	id: string;
	kind: 'agent' | 'person';
	name: string;
	/** Stored without `@`. */
	handle: string;
	status: Exclude<EmployeeStatus, 'inactive'>;
	/** Required on a valid record; missing is shown as a problem, never as "No team". */
	homeTeam?: TeamRef;
	/** Explicit `reports_to_employee_id`; undefined is No explicit manager. */
	reportsToId?: string;
	/** Teams this employee leads: a second, separate source of responsibility (REPORT-5). */
	leads: TeamRef[];
	/** Agents only: the executing office and when it last reported, `null` for Unplaced. Undefined for a person. */
	office?: Placement | null;
	/** A person with an account (§7.4); never true for an agent. */
	canSignIn: boolean;
	/** The employee record's revision, sent back with a change (REPORT-4). */
	revision: number;
}

export interface OrgChart {
	organization: { id: string; name: string };
	/** The person who owns the organization: an employee in `employees`, with a home team like anyone. */
	ownerId: string;
	/** The signed-in account's person in this organization, when it has one. */
	viewerId?: string;
	/** Every active employee, people and agents, the owner's person and the HR lead included. */
	employees: ChartEmployee[];
}
