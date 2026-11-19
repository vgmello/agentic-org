import { describe, expect, it } from 'vitest';
import { AGENT_SETUP_URL, agentSetupHref, profileHref } from './urls';

describe('urls', () => {
	it('places Agent setup in Organization settings and encodes member IDs', () => {
		expect(AGENT_SETUP_URL).toBe('/settings/agents');
		expect(agentSetupHref('kai')).toBe('/settings/agents/kai');
		expect(agentSetupHref('a/b')).toBe('/settings/agents/a%2Fb');
		expect(profileHref('a b')).toBe('/members/a%20b');
	});
});
