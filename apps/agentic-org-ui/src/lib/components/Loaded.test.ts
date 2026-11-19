import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import { unavailableCall } from '$lib/platform/unavailable-api';
import Loaded from './Loaded.svelte';
import { Resource } from './resource.svelte';

async function renderUnavailable(location: { place?: string; area?: string }) {
	const resource = new Resource<unknown>(unavailableCall);
	await resource.reload();
	return render(Loaded, {
		props: {
			resource,
			label: 'roles',
			error: "Couldn't load the roles",
			children: createRawSnippet(() => ({ render: () => '<p>Roles content</p>' })),
			...location
		}
	});
}

test.each([{}, { place: 'Roles' }, { area: 'Organization settings' }])(
	'uses an inline unavailable message with incomplete location %j',
	async (location) => {
		const { body, head } = await renderUnavailable(location);
		expect(body).toContain('Not available yet');
		expect(head).not.toContain('<title>');
	}
);

test('uses the route placeholder with both place and area', async () => {
	const { body, head } = await renderUnavailable({ place: 'Roles', area: 'Organization settings' });
	expect(body).toContain("Roles isn't built yet");
	expect(head).toContain('<title>Roles · Organization settings · Agentic Org</title>');
});
