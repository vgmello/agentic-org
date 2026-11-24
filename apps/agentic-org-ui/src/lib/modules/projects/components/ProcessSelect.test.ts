import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import { Resource } from '$lib/components/resource.svelte';
import type { processesApi } from '$lib/modules/processes';
import ProcessSelect from './ProcessSelect.svelte';

test('ProcessSelect notes when development processes are unsupported in this version', async () => {
	const catalogueResource = new Resource<Awaited<ReturnType<typeof processesApi.devProcesses>>>(
		async () => {
			throw {
				source: 'api',
				code: 'unsupported',
				message: 'This office release does not serve development processes.'
			};
		}
	);
	await catalogueResource.reload();
	const { body } = render(ProcessSelect, {
		props: {
			id: 'dev-process',
			organizationId: 'org-1',
			projectId: null,
			value: '',
			catalogueResource
		}
	});
	expect(body).toContain("Development processes aren't available in this version");
	expect(body).toContain('only None can be chosen');
	expect(body).not.toContain('Retry');
});
