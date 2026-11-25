import { appSession } from './app-session.svelte';
import { drafts } from './draft-registry';
import { createUnsavedQuery } from './unsaved-query';

export type SignOutFlowDeps = {
	isSignedIn: () => boolean;
	sessionGeneration: () => number;
	onSessionEnded: (listener: () => void) => () => void;
	signOut: () => Promise<void>;
	anyUnsaved: () => Promise<boolean>;
	listen?: () => void;
};

/**
 * The tab's sign-out flow (SIGN-19, SIGN-29): asks first when any open tab of this HQ has
 * unsaved text, shows "Signing out…" while HQ ends the session, and keeps the session when
 * HQ can't be reached. Every Sign out control in the page calls `request`.
 */
export function createSignOutFlow(deps: SignOutFlowDeps) {
	let confirming = $state(false);
	let signingOut = $state(false);
	let failed = $state(false);
	let queryingGeneration: number | undefined;
	let runToken = 0;

	const sameActiveSession = (generation: number) =>
		deps.isSignedIn() && deps.sessionGeneration() === generation;

	deps.onSessionEnded(() => {
		confirming = false;
		signingOut = false;
		failed = false;
		queryingGeneration = undefined;
		runToken++;
	});

	async function run(generation = deps.sessionGeneration()) {
		if (signingOut || !sameActiveSession(generation)) return;
		const token = ++runToken;
		confirming = false;
		signingOut = true;
		failed = false;
		try {
			await deps.signOut();
		} catch {
			if (sameActiveSession(generation)) failed = true;
		} finally {
			if (runToken === token) signingOut = false;
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
			deps.listen?.();
		},
		async request() {
			if (!deps.isSignedIn()) return;
			const generation = deps.sessionGeneration();
			if (signingOut || confirming || queryingGeneration === generation) return;
			queryingGeneration = generation;
			let hasUnsaved: boolean;
			try {
				hasUnsaved = await deps.anyUnsaved();
			} finally {
				if (queryingGeneration === generation) queryingGeneration = undefined;
			}
			if (!sameActiveSession(generation)) return;
			if (hasUnsaved) confirming = true;
			else await run(generation);
		},
		confirm: () => run(deps.sessionGeneration()),
		stay() {
			confirming = false;
		},
		dismissFailure() {
			failed = false;
		}
	};
}

function compose() {
	let query: ReturnType<typeof createUnsavedQuery> | undefined;
	const ready = () =>
		(query ??= createUnsavedQuery({
			channel: new BroadcastChannel('org-drafts'),
			local: drafts.hasUnsavedText
		}));
	return createSignOutFlow({
		isSignedIn: () => appSession.state.kind === 'signed-in',
		sessionGeneration: () => appSession.sessionGeneration,
		onSessionEnded: (listener) => appSession.onSessionEnded(() => listener()),
		signOut: () => appSession.controller().signOut(),
		anyUnsaved: () => ready().anyUnsaved(),
		listen: () => {
			ready();
		}
	});
}

export const signOutFlow = compose();
