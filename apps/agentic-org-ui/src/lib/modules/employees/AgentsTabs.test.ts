import type { ComponentProps } from 'svelte';
import { render } from 'svelte/server';
import AgentsTabs from './AgentsTabs.svelte';
import { expect, expectTypeOf, test } from 'vitest';

type AgentsTabsProps = ComponentProps<typeof AgentsTabs>;

test('AGENT-3 TEAM-11 S2: current is required and accepts only the supported views', () => {
	expectTypeOf<{ current: 'agents' }>().toExtend<AgentsTabsProps>();
	expectTypeOf<{ current: 'org-chart' }>().toExtend<AgentsTabsProps>();
	expectTypeOf<{ current: 'settings' }>().not.toExtend<AgentsTabsProps>();
	expectTypeOf<{}>().not.toExtend<AgentsTabsProps>();
});

test('AGENT-3 TEAM-11 S2: a JavaScript caller missing current does not guess an active view', () => {
	// JavaScript can bypass the required TypeScript prop; do not invent a current route.
	const { body } = render(AgentsTabs, { props: {} as AgentsTabsProps });
	expect(body).not.toContain('aria-current="page"');
});
