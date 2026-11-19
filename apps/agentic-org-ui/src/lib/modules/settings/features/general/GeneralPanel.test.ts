import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import GeneralPanel from './GeneralPanel.svelte';

test('General starts with a five-row loading skeleton', () => {
	const { body } = render(GeneralPanel, { props: { organizationId: 'org' } });
	expect(body).toContain('Loading settings');
	expect(body.match(/<li\b/g)).toHaveLength(5);
});
