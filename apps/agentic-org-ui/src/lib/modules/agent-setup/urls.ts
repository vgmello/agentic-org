// Where Agent setup lives (app-shell-ux §0.1: Organization settings › Agent setup).
export const AGENT_SETUP_URL = '/settings/agents';

/** One agent's setup, by its stable member ID. */
export const agentSetupHref = (memberId: string) =>
	`${AGENT_SETUP_URL}/${encodeURIComponent(memberId)}`;

/** The colleague profile that View profile returns to. */
export const profileHref = (memberId: string) => `/members/${encodeURIComponent(memberId)}`;
