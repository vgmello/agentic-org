import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpHomeApi } from './http-api';
import type { HomeProject, MonthSummary, WorkingNow } from './types';

/**
 * What Home reads beside the Inbox and Activity: project cards, who is working now, and the month line.
 * Each is its own call, so each Home section loads, fails and retries on its own.
 */
export interface HomeApi {
	projects(organizationId: string): Promise<HomeProject[]>;
	workingNow(organizationId: string): Promise<WorkingNow>;
	/** Null when costs are not served or the viewer may not see amounts: Home then omits the line. */
	month(organizationId: string): Promise<MonthSummary | null>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayHomeApi = createHttpHomeApi({ orgApi });

/**
 * Production Home API backed by live organization snapshots (`orgApi.view`).
 * Fails closed when no signed-in gateway session exists.
 * Under `bun run dev:mock`, `e2e/fakes/modules/home.ts` stands in for this file.
 */
export const homeApi: HomeApi = Object.freeze<HomeApi>({
	projects: (organizationId) =>
		signedInSession() ? gatewayHomeApi.projects(organizationId) : unavailableCall(),
	workingNow: (organizationId) =>
		signedInSession() ? gatewayHomeApi.workingNow(organizationId) : unavailableCall(),
	month: (organizationId) =>
		signedInSession() ? gatewayHomeApi.month(organizationId) : unavailableCall()
});
