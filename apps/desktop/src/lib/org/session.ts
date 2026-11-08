import type { CommandAccepted, OrgSnapshot } from '$lib/proto';
import type { OrgApi } from './api';
import type { OrgCommand, OrgCommandInput } from './commands';

/** An organization's structure and commands, bound to one organization, for the org chart's actions. */
export interface OrgSession {
	/** The current structure (fresh from the office each time). */
	snapshot(): Promise<OrgSnapshot>;
	/** Ask HQ to change it. Rejects with HQ's error. */
	command(input: OrgCommandInput | OrgCommand): Promise<CommandAccepted>;
}

export const sessionFor = (api: OrgApi, organizationId: string): OrgSession => ({
	snapshot: async () => (await api.view(organizationId)).snapshot,
	command: (input) => api.command(organizationId, input)
});
