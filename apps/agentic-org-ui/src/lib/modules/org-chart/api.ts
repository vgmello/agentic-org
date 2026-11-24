import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { membersApi } from '$lib/modules/members/api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpOrgChartApi } from './http-api';
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

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayOrgChartApi = createHttpOrgChartApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	membersApi
});

// Production org-chart API backed by live org snapshot and `members/set-reports-to`.
// Under `bun run dev:mock`, e2e/fakes/modules/org-chart.ts stands in for this file.
export const orgChartApi: OrgChartApi = Object.freeze<OrgChartApi>({
	chart: (orgId) => (signedInSession() ? gatewayOrgChartApi.chart(orgId) : unavailableCall()),
	changeManager: (orgId, employeeId, managerId, expectedRevision) =>
		signedInSession()
			? gatewayOrgChartApi.changeManager(orgId, employeeId, managerId, expectedRevision)
			: unavailableCall()
});
