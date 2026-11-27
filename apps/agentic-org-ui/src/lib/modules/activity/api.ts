import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { inboxApi } from '$lib/modules/inbox/api';
import { orgApi } from '$lib/modules/organization/api';
import { workItemsApi } from '$lib/modules/workitems/api';
import { createHttpActivityApi } from './http-api';
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
	feed(
		organizationId: string,
		query: ActivityQuery,
		limit: number,
		signal?: AbortSignal
	): Promise<ActivityPage>;
	/** The teams, members and projects the filters offer. */
	options(organizationId: string): Promise<ActivityOptions>;
	/** One authorized thread, read-only unless the owner is a participant. */
	thread(organizationId: string, threadId: string): Promise<ActivityThread>;
	/** New rows as they arrive; resolves to the function that stops listening. */
	subscribe(organizationId: string, onRow: (row: ActivityRow) => void): Promise<() => void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

function subscribeActivityFeeds(organizationId: string, onEvent: () => void): () => void {
	const session = signedInSession();
	if (!session) return () => {};
	const orgScope = {
		kind: 'organization' as const,
		instance_id: session.instance_id,
		org_id: organizationId
	};
	const officeId = session.organizations.find((o) => o.org_id === organizationId)?.office_ids[0];
	const subscriptions = [
		{ scope: orgScope, feed: 'organization' as const },
		...(officeId
			? ([
					{
						scope: {
							kind: 'office' as const,
							instance_id: session.instance_id,
							org_id: organizationId,
							office_id: officeId
						},
						feed: 'work' as const
					},
					{
						scope: {
							kind: 'office' as const,
							instance_id: session.instance_id,
							org_id: organizationId,
							office_id: officeId
						},
						feed: 'messaging' as const
					},
					{
						scope: {
							kind: 'office' as const,
							instance_id: session.instance_id,
							org_id: organizationId,
							office_id: officeId
						},
						feed: 'presence' as const
					}
				] as const)
			: [])
	];
	return appSession.controller().subscribeEvents(subscriptions, {
		onChange: () => onEvent(),
		onResync: () => onEvent()
	});
}

const gatewayActivityApi = createHttpActivityApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	workItemsApi,
	inboxApi,
	viewerId: (organizationId) =>
		signedInSession()?.organizations.find((o) => o.org_id === organizationId)?.employee_id,
	subscribe: subscribeActivityFeeds
});

// Production routes through the signed-in browser gateway and fails closed when signed out.
// Under `bun run dev:mock`, `e2e/fakes/modules/activity.ts` stands in for this file.
export const activityApi: ActivityApi = Object.freeze<ActivityApi>({
	feed: (organizationId, query, limit, signal) =>
		signedInSession()
			? gatewayActivityApi.feed(organizationId, query, limit, signal)
			: unavailableCall(),
	options: (organizationId) =>
		signedInSession() ? gatewayActivityApi.options(organizationId) : unavailableCall(),
	thread: (organizationId, threadId) =>
		signedInSession() ? gatewayActivityApi.thread(organizationId, threadId) : unavailableCall(),
	subscribe: (organizationId, onRow) =>
		signedInSession() ? gatewayActivityApi.subscribe(organizationId, onRow) : unavailableCall()
});
