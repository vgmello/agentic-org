import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgChart } from './types';

/**
 * What the Org chart reads and changes (offices-and-teams-ux §7.8). Errors are `AppError`s. Hand over ownership is
 * the members module's `handOverOwnership`; adding a person and moving a home team are the teams and members
 * modules' own calls.
 */
export interface OrgChartApi {
	/** The owner and every active employee with their explicit reporting line. */
	chart(organizationId: string): Promise<OrgChart>;
	/**
	 * Change manager (REPORT-3/4): records `managerId` as the employee's explicit manager, or none with `null`.
	 * Checked against `expectedRevision`: refuses `conflict` when the record changed since, or when the choice
	 * would make a reporting loop (the message names what changed); `not_found` when either is gone. Team leads are
	 * never changed by it.
	 */
	changeManager(
		organizationId: string,
		employeeId: string,
		managerId: string | null,
		expectedRevision: number
	): Promise<void>;
}

// The web API does not serve these yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/org-chart.ts stands in for this file.
export const orgChartApi: OrgChartApi = {
	chart: unavailableCall,
	changeManager: unavailableCall
};
