import { unavailableCall } from '$lib/platform/unavailable-api';
import type { AccessOverview } from './types';

/**
 * What Members and access reads. The sign-in actions are the members module's (`membersApi.inviteToSignIn`,
 * `newSignInLink`, `removeAccess`), used through its SignInAccessDialogs, so the profile and this page share one
 * contract. Errors are `AppError`s.
 */
export interface AccessApi {
	overview(organizationId: string): Promise<AccessOverview>;
}

// The web API does not serve this yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/access.ts stands in for this file.
export const accessApi: AccessApi = {
	overview: unavailableCall
};
