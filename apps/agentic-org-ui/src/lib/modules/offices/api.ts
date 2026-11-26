import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { createHttpOfficesOverviewApi } from './overview-http-api';
import type { AddProgress, Issued, OfficesOverview } from './types';

/** The office flows beyond the connected office: add, approve, remove, rename, place and the outboxes. */
export interface OfficesApi {
	overview(organizationId: string): Promise<OfficesOverview>;

	/** Approves a join with the check code typed at HQ. Rejects `code_mismatch` (with `triesLeft`) or `join_cancelled`. */
	approveJoin(organizationId: string, requestId: string, code: string): Promise<void>;
	declineJoin(organizationId: string, requestId: string): Promise<void>;

	/** An invitation for another machine. Replacing one cancels the previous. */
	invite(organizationId: string, name: string): Promise<Issued>;
	newInvitation(organizationId: string, invitationId: string): Promise<Issued>;
	cancelInvitation(organizationId: string, invitationId: string): Promise<void>;
	removeInvitation(organizationId: string, invitationId: string): Promise<void>;

	/** Add office › On this instance: starts it and answers an operation to watch. */
	addOnInstance(organizationId: string, name: string): Promise<{ operationId: string }>;
	addProgress(organizationId: string, operationId: string): Promise<AddProgress>;
	/** Cancels the same operation. `false`: too late, it is being added now. */
	cancelAdd(organizationId: string, operationId: string): Promise<boolean>;

	removeOffice(organizationId: string, officeId: string): Promise<void>;
	renameOffice(organizationId: string, officeId: string, name: string): Promise<void>;
	placeAgent(organizationId: string, memberId: string, officeId: string): Promise<void>;
	retryOutbox(organizationId: string, itemId: string): Promise<void>;
	discardOutbox(organizationId: string, itemId: string): Promise<void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayOfficesOverviewApi = createHttpOfficesOverviewApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal)
	}
});

// `overview` reads live offices from `/api/v1/organizations/{org}/offices` when signed in and fails
// closed when signed out. Remaining office management mutations stay unavailable until their web
// routes are wired. Under `bun run dev:mock`, e2e/fakes/modules/offices.ts stands in.
export const officesApi: OfficesApi = {
	overview: (organizationId) =>
		signedInSession() ? gatewayOfficesOverviewApi.overview(organizationId) : unavailableCall(),
	approveJoin: unavailableCall,
	declineJoin: unavailableCall,
	invite: unavailableCall,
	newInvitation: unavailableCall,
	cancelInvitation: unavailableCall,
	removeInvitation: unavailableCall,
	addOnInstance: unavailableCall,
	addProgress: unavailableCall,
	cancelAdd: unavailableCall,
	removeOffice: unavailableCall,
	renameOffice: unavailableCall,
	placeAgent: unavailableCall,
	retryOutbox: unavailableCall,
	discardOutbox: unavailableCall
};
