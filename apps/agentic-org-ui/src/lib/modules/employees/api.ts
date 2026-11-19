import { unavailableCall } from '$lib/platform/unavailable-api';
import type { HireAccess, HireRequest } from './types';

/** Who may hire, and the hire requests list (our-org-in-the-app §7.1). */
export interface EmployeesApi {
	hireAccess(organizationId: string): Promise<HireAccess>;
	hireRequests(organizationId: string): Promise<HireRequest[]>;
	/** A drafting request: nothing has been created. */
	withdraw(organizationId: string, requestId: string): Promise<void>;
	/** Finishes a hire that stopped partway, without a new confirmation. */
	retry(organizationId: string, requestId: string): Promise<void>;
}

// The web API does not serve these yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/employees.ts stands in for this file.
export const employeesApi: EmployeesApi = {
	hireAccess: unavailableCall,
	hireRequests: unavailableCall,
	withdraw: unavailableCall,
	retry: unavailableCall
};
