import { unavailableCall } from '$lib/platform/unavailable-api';
import type { AgentSetupDetail, AgentSetupList, ExecutionEdit, RoleEdit } from './types';

/**
 * What Organization settings › Agent setup reads and changes. Errors are `AppError`s: `not_found` from `agent` when
 * the member is gone or isn't an agent; `conflict` from the saves when the revision moved (the error carries the
 * newer setup as `latest`); `not_authorized` for a viewer who may not change agent setup. Pause, Resume and
 * Offboard are the organization's own member commands (`$lib/modules/organization/actions`), not calls here.
 */
export interface AgentSetupApi {
	/** Every current agent (active and paused), with its configuration, placement and readiness. */
	list(organizationId: string): Promise<AgentSetupList>;
	agent(organizationId: string, memberId: string): Promise<AgentSetupDetail>;
	/** Writes `execution.md` (profile and model) only; the employee record is untouched. */
	saveExecution(
		organizationId: string,
		memberId: string,
		edit: ExecutionEdit
	): Promise<AgentSetupDetail>;
	/** Writes the employee record's `role_id` only; execution is untouched. */
	setRole(organizationId: string, memberId: string, edit: RoleEdit): Promise<AgentSetupDetail>;
}

// The web API does not serve these yet: every call refuses, and the screens say so.
// Under `bun run dev:mock`, e2e/fakes/modules/agent-setup.ts stands in for this file.
export const agentSetupApi: AgentSetupApi = {
	list: unavailableCall,
	agent: unavailableCall,
	saveExecution: unavailableCall,
	setRole: unavailableCall
};
