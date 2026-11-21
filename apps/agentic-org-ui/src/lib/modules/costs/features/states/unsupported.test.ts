import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import { Resource } from '$lib/components/resource.svelte';
import AgentBudgets from '../agent-budgets/AgentBudgets.svelte';
import OrganizationCost from '../organization-cost/OrganizationCost.svelte';
import PricesSection from '../prices/PricesSection.svelte';

async function unsupportedResource<T>(message: string): Promise<Resource<T>> {
	const resource = new Resource<T>(async () => {
		throw { source: 'api', code: 'unsupported', message };
	});
	await resource.reload();
	return resource;
}

test('AgentBudgets renders unsupported without a retry button', async () => {
	const resource = await unsupportedResource<never>('This office release omits agent budgets.');
	const { body } = render(AgentBudgets, {
		props: { resource, memberHref: (id) => `/teams/members/${id}/cost` }
	});
	expect(body).toContain('Not available in this version');
	expect(body).toContain('This office release omits agent budgets.');
	expect(body).not.toContain('Try again');
	expect(body).not.toContain('Retry');
});

test('OrganizationCost renders unsupported without period menu or retry button', async () => {
	const resource = await unsupportedResource<never>('This office release omits org costs.');
	const { body } = render(OrganizationCost, {
		props: {
			resource,
			period: { kind: 'this_month', timeZone: 'UTC' },
			onperiodchange: () => {},
			onopenrecords: () => {}
		}
	});
	expect(body).toContain('Not available in this version');
	expect(body).toContain('This office release omits org costs.');
	expect(body).not.toContain('Try again');
	expect(body).not.toContain('Retry');
	expect(body).not.toContain('This month');
});

test('PricesSection renders unsupported without a retry button', async () => {
	const resource = await unsupportedResource<never>('This office release omits price tables.');
	const { body } = render(PricesSection, {
		props: { resource, zone: 'UTC', onadd: () => {} }
	});
	expect(body).toContain('Not available in this version');
	expect(body).toContain('This office release omits price tables.');
	expect(body).not.toContain('Try again');
	expect(body).not.toContain('Retry');
});
