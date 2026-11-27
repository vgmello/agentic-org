import type { CommandAccepted, OrgFeedNotice, OrgView } from '$lib/proto';
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgCommand, OrgCommandInput } from './commands';
import { createHttpOrgApi } from './http-api';

/** What the org chart needs from an office: `org.snapshot`, and the commands that change the structure. */
export interface OrgApi {
	/** The structure of an organization, and whether it is the TEMPORARY invented sample. */
	view(organizationId: string, signal?: AbortSignal): Promise<OrgView>;
	/**
	 * Ask HQ to change the organization. Resolves with its answer, or rejects with the error HQ gave (an
	 * `AppError` with source `api`). A repeated `command_id` gets the same answer and changes nothing.
	 */
	command(organizationId: string, input: OrgCommandInput | OrgCommand): Promise<CommandAccepted>;
	/** Be told when an organization's structure changes (anyone's command, ours included) and whether the feed is live. Resolves to the function that stops listening. */
	onFeed(listener: (notice: OrgFeedNotice) => void): Promise<() => void>;
	/** "Live updates paused" has a Retry: connect the live feed now. Resolves when asked, not when it is back (a `live` notice says that). */
	retryFeed(organizationId: string): Promise<void>;
	/** Whether the feed is live now (`undefined`: it has not said). For a screen that was not listening when that changed. */
	feedLive(organizationId: string): Promise<boolean | undefined>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayOrgApi = createHttpOrgApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	instanceId: () => signedInSession()?.instance_id,
	newId: () => appSession.newOperationId()
});

/**
 * Production organization API backed by the browser gateway (`/api/v1/organizations/{org}/...`).
 * Fails closed when no signed-in gateway session exists and never falls back to a browser reducer.
 */
export const orgApi: OrgApi = Object.freeze<OrgApi>({
	view: (organizationId: string, signal?: AbortSignal) =>
		signedInSession() ? gatewayOrgApi.view(organizationId, signal) : unavailableCall(),
	command: (organizationId: string, input: OrgCommandInput | OrgCommand) =>
		signedInSession() ? gatewayOrgApi.command(organizationId, input) : unavailableCall(),
	onFeed: () => (signedInSession() ? Promise.resolve(() => {}) : unavailableCall()),
	retryFeed: () => (signedInSession() ? Promise.resolve() : unavailableCall()),
	feedLive: () => (signedInSession() ? Promise.resolve(true) : unavailableCall())
});
