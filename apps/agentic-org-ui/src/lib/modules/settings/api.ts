import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpSettingsApi } from './http-api';
import type { GeneralSettings } from './types';

/**
 * What Organization settings › General reads and changes. Costs has its own module (`$lib/modules/costs`), Members
 * and access too (`$lib/modules/access`); the sign-in actions belong to the members module.
 */
export interface SettingsApi {
	general(organizationId: string, signal?: AbortSignal): Promise<GeneralSettings>;
	/** Makes `memberId`'s account the administrator, confirmed with the administrator's own password. */
	handOverAdministrator(
		organizationId: string,
		memberId: string,
		password: string
	): Promise<GeneralSettings>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewaySettingsApi = createHttpSettingsApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	session: () => {
		const s = signedInSession();
		return s ? { accountAdmin: s.admin, login: s.account.login } : undefined;
	}
});

// Production settings API backed by `/api/v1/instance`, `/api/v1/organizations`, and org snapshot/access routes.
// Under `bun run dev:mock`, e2e/fakes/modules/settings.ts stands in for this file.
export const settingsApi: SettingsApi = Object.freeze<SettingsApi>({
	general: (organizationId, signal) =>
		signedInSession() ? gatewaySettingsApi.general(organizationId, signal) : unavailableCall(),
	handOverAdministrator: (organizationId, memberId, password) =>
		signedInSession()
			? gatewaySettingsApi.handOverAdministrator(organizationId, memberId, password)
			: unavailableCall()
});
