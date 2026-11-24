import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpAccessApi } from './http-api';
import type { AccessOverview } from './types';

/**
 * What Members and access reads. The sign-in actions are the members module's (`membersApi.inviteToSignIn`,
 * `newSignInLink`, `removeAccess`), used through its SignInAccessDialogs, so the profile and this page share one
 * contract. Errors are `AppError`s.
 */
export interface AccessApi {
	overview(organizationId: string): Promise<AccessOverview>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayAccessApi = createHttpAccessApi({
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

// Production access API backed by `/api/v1/organizations/{org}/access` and org snapshot.
// Under `bun run dev:mock`, e2e/fakes/modules/access.ts stands in for this file.
export const accessApi: AccessApi = Object.freeze<AccessApi>({
	overview: (orgId) => (signedInSession() ? gatewayAccessApi.overview(orgId) : unavailableCall())
});
