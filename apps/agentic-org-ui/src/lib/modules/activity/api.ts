import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	ActivityOptions,
	ActivityPage,
	ActivityQuery,
	ActivityRow,
	ActivityThread
} from './types';

/** The merged, authorized Activity feed (our-org-in-the-app §6, §6.1). */
export interface ActivityApi {
	/** Matching rows newest first, `limit` at a time, and how many older ones remain. */
	feed(organizationId: string, query: ActivityQuery, limit: number): Promise<ActivityPage>;
	/** The teams, members and projects the filters offer. */
	options(organizationId: string): Promise<ActivityOptions>;
	/** One authorized thread, read-only unless the owner is a participant. */
	thread(organizationId: string, threadId: string): Promise<ActivityThread>;
	/** New rows as they arrive; resolves to the function that stops listening. */
	subscribe(organizationId: string, onRow: (row: ActivityRow) => void): Promise<() => void>;
}

// The web API does not serve the feed yet: every call refuses and screens say so. Under `bun run dev:mock`,
// e2e/fakes/modules/activity.ts stands in for this file.
export const activityApi: ActivityApi = {
	feed: unavailableCall,
	options: unavailableCall,
	thread: unavailableCall,
	subscribe: unavailableCall
};
