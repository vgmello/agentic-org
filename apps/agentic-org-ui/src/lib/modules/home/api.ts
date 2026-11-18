import { unavailableCall } from '$lib/platform/unavailable-api';
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

// The web API does not serve these yet: every call refuses and Home's sections say so. Under
// `bun run dev:mock`, e2e/fakes/modules/home.ts stands in for this file.
export const homeApi: HomeApi = {
	projects: unavailableCall,
	workingNow: unavailableCall,
	month: unavailableCall
};
