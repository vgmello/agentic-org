import { unavailableCall } from '$lib/platform/unavailable-api';
import type { GeneralSettings } from './types';

/**
 * What Organization settings › General reads and changes. Costs has its own module (`$lib/modules/costs`), Members
 * and access too (`$lib/modules/access`); the sign-in actions belong to the members module.
 */
export interface SettingsApi {
	general(organizationId: string): Promise<GeneralSettings>;
	/** Makes `memberId`'s account the administrator, confirmed with the administrator's own password. */
	handOverAdministrator(
		organizationId: string,
		memberId: string,
		password: string
	): Promise<GeneralSettings>;
}

// The web API does not serve these yet: every call refuses, and each screen says so.
// Under `bun run dev:mock`, e2e/fakes/modules/settings.ts stands in for this file.
export const settingsApi: SettingsApi = {
	general: unavailableCall,
	handOverAdministrator: unavailableCall
};
