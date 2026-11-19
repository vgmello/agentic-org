import { unavailableCall } from '$lib/platform/unavailable-api';

/** The signed-in account's preferences that the app keeps for it (not the keyboard switch: that is device-wide). */
export interface PreferencesApi {
	/** The saved viewing zone, or null when the account has none yet. */
	timeZone(organizationId: string): Promise<{ saved: string | null }>;
	/** Saves the viewing zone (a valid IANA name). Rejects with the reason when it is refused. */
	saveTimeZone(organizationId: string, zone: string): Promise<void>;
}

// The web API has no account preference contract yet: every call refuses, and time shows in the detected zone.
// Under `bun run dev:mock`, e2e/fakes/modules/preferences.ts stands in for this file.
export const preferencesApi: PreferencesApi = {
	timeZone: unavailableCall,
	saveTimeZone: unavailableCall
};
