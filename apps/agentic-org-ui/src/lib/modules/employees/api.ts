import { unavailableCall } from '$lib/platform/unavailable-api';
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

// The web API does not serve these yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/employees.ts stands in for this file.
export const employeesApi: EmployeesApi = {
	hireAccess: unavailableCall,
	hireChoices: unavailableCall,
	hireRequests: unavailableCall,
	hireRequest: unavailableCall,
	draft: unavailableCall,
	preview: unavailableCall,
	confirm: unavailableCall,
	withdraw: unavailableCall,
	retry: unavailableCall
};
