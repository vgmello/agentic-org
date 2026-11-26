import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { createHttpPreferencesApi } from './http-api';

/** The signed-in account's preferences that the app keeps for it (not the keyboard switch: that is device-wide). */
export interface PreferencesApi {
	/** The saved viewing zone, or null when the account has none yet. */
	timeZone(organizationId: string): Promise<{ saved: string | null }>;
	/** Saves the viewing zone (a valid IANA name). Rejects with the reason when it is refused. */
	saveTimeZone(organizationId: string, zone: string): Promise<void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayPreferencesApi = createHttpPreferencesApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	sessionTimeZone: () => signedInSession()?.account.time_zone
});

// Production preferences API backed by `GET /api/v1/session` and `POST /api/v1/account/preferences`.
// Under `bun run dev:mock`, e2e/fakes/modules/preferences.ts stands in for this file.
export const preferencesApi: PreferencesApi = Object.freeze<PreferencesApi>({
	timeZone: (organizationId) =>
		signedInSession() ? gatewayPreferencesApi.timeZone(organizationId) : unavailableCall(),
	saveTimeZone: (organizationId, zone) =>
		signedInSession() ? gatewayPreferencesApi.saveTimeZone(organizationId, zone) : unavailableCall()
});
