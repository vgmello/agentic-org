import { appSession } from './app-session.svelte';
import { drafts } from './draft-registry';
import { createUnsavedQuery } from './unsaved-query';

/**
 * The tab's sign-out flow (SIGN-19, SIGN-29): asks first when any open tab of this HQ has
 * unsaved text, shows "Signing out…" while HQ ends the session, and keeps the session when
 * HQ can't be reached. Every Sign out control in the page calls `request`.
 */
function compose() {
	let confirming = $state(false);
	let signingOut = $state(false);
	let failed = $state(false);
	let query: ReturnType<typeof createUnsavedQuery> | undefined;
	const ready = () =>
		(query ??= createUnsavedQuery({ channel: new BroadcastChannel('org-drafts'), local: drafts.hasUnsavedText }));
	const unsaved = () => ready().anyUnsaved();
	async function run() {
		if (signingOut) return;
		confirming = false;
		signingOut = true;
		failed = false;
		try {
			await appSession.controller().signOut();
		} catch {
			failed = true;
		} finally {
			signingOut = false;
		}
	}
	return {
		get confirming() {
			return confirming;
		},
		get signingOut() {
			return signingOut;
		},
		get failed() {
			return failed;
		},
		/** Starts this tab answering other tabs' unsaved-text queries. */
		listen() {
			ready();
		},
		async request() {
			if (signingOut || confirming) return;
			if (await unsaved()) confirming = true;
			else await run();
		},
		confirm: run,
		stay() {
			confirming = false;
		},
		dismissFailure() {
			failed = false;
		}
	};
}

export const signOutFlow = compose();
