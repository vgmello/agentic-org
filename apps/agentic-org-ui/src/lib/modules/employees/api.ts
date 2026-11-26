import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpEmployeesApi } from './http-api';
import type { HireAccess, HireChoices, HirePreview, HireProposal, HireRequest } from './types';

/**
 * Hiring an agent (our-org-in-the-app §7, §7.1). Every call checks that the caller may hire into the team named and
 * refuses with `not_authorized` otherwise; a hire into a full team is refused with `team_full` and the limit named.
 */
export interface EmployeesApi {
	hireAccess(organizationId: string): Promise<HireAccess>;
	hireChoices(organizationId: string): Promise<HireChoices>;
	/** The owner's view holds every request; anyone else's only its own. Newest first. */
	hireRequests(organizationId: string): Promise<HireRequest[]>;
	hireRequest(organizationId: string, requestId: string): Promise<HireRequest>;
	/**
	 * Asks the HR lead to draft a proposal: records a drafting request and starts one HR turn. With `requestId`, asks
	 * again for that request (Try again after the HR lead stopped).
	 */
	draft(
		organizationId: string,
		input: { reason: string; homeTeamId: string; requestId?: string }
	): Promise<HireRequest>;
	/** The dry run for Review: creates nothing. */
	preview(organizationId: string, proposal: HireProposal): Promise<HirePreview>;
	/**
	 * The hirer's confirmation: the office provisions at once. Without `requestId` the call carries every required
	 * field and is its own confirmation (a proposal filled in by hand).
	 */
	confirm(
		organizationId: string,
		input: { requestId?: string; reason: string; proposal: HireProposal }
	): Promise<HireRequest>;
	/** A drafting request: nothing has been created. */
	withdraw(organizationId: string, requestId: string): Promise<void>;
	/** Finishes a hire that stopped partway, without a new confirmation. */
	retry(organizationId: string, requestId: string): Promise<void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayEmployeesApi = createHttpEmployeesApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	session: () => {
		const s = signedInSession();
		return s ? { accountAdmin: s.admin } : undefined;
	}
});

// Production HR hiring API backed by `/api/v1/organizations/{org}/hires/*`.
// Under `bun run dev:mock`, e2e/fakes/modules/employees.ts stands in for this file.
export const employeesApi: EmployeesApi = Object.freeze<EmployeesApi>({
	hireAccess: (organizationId) =>
		signedInSession() ? gatewayEmployeesApi.hireAccess(organizationId) : unavailableCall(),
	hireChoices: (organizationId) =>
		signedInSession() ? gatewayEmployeesApi.hireChoices(organizationId) : unavailableCall(),
	hireRequests: (organizationId) =>
		signedInSession() ? gatewayEmployeesApi.hireRequests(organizationId) : unavailableCall(),
	hireRequest: (organizationId, requestId) =>
		signedInSession()
			? gatewayEmployeesApi.hireRequest(organizationId, requestId)
			: unavailableCall(),
	draft: (organizationId, input) =>
		signedInSession() ? gatewayEmployeesApi.draft(organizationId, input) : unavailableCall(),
	preview: (organizationId, proposal) =>
		signedInSession() ? gatewayEmployeesApi.preview(organizationId, proposal) : unavailableCall(),
	confirm: (organizationId, input) =>
		signedInSession() ? gatewayEmployeesApi.confirm(organizationId, input) : unavailableCall(),
	withdraw: (organizationId, requestId) =>
		signedInSession() ? gatewayEmployeesApi.withdraw(organizationId, requestId) : unavailableCall(),
	retry: (organizationId, requestId) =>
		signedInSession() ? gatewayEmployeesApi.retry(organizationId, requestId) : unavailableCall()
});
