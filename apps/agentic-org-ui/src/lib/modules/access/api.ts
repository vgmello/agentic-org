import { unavailableCall } from '$lib/platform/unavailable-api';
import type { AccessOverview, IssuedLink } from './types';

/**
 * What Members and access reads and changes. Errors are `AppError`s: `not_authorized` for anyone but the owner or the
 * administrator, `not_found` for a person who is no longer in the organization, and `conflict` for removing the owner.
 */
export interface AccessApi {
	overview(organizationId: string): Promise<AccessOverview>;
	/** Invite to sign in (no account) or New sign-in link (has one). Inviting again cancels the previous link. */
	issueLink(
		organizationId: string,
		memberId: string,
		kind: IssuedLink['kind']
	): Promise<IssuedLink>;
	/** Signs the person out everywhere. Refused for the owner ("Hand over ownership first."). */
	removeAccess(organizationId: string, memberId: string): Promise<void>;
}

// The web API does not serve these yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/access.ts stands in for this file.
export const accessApi: AccessApi = {
	overview: unavailableCall,
	issueLink: unavailableCall,
	removeAccess: unavailableCall
};
