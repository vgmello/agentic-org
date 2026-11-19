import { describe, expect, it } from 'vitest';
import { officeApi, toAppError } from '$lib/office/api';
import { orgApi } from '$lib/org/api';
import { sender, workApi } from '$lib/work';

describe('production APIs before concrete gateway adapters exist', () => {
	it('refuses office reads and writes rather than creating sample connections or starting services', async () => {
		for (const result of [officeApi.connections(), officeApi.ensureRunning(), officeApi.createOrganization('Acme'), officeApi.onOfficeStatus(() => {})])
			await expect(result).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		expect('loginItem' in officeApi).toBe(false);
		expect('openLoginSettings' in officeApi).toBe(false);
	});
	it('refuses organization reads, writes and subscriptions without a browser reducer fallback', async () => {
		for (const result of [orgApi.view('org'), orgApi.command('org', { method: 'team.create', params: { team_id: 'team', name: 'Ops' } }), orgApi.onFeed(() => {})])
			await expect(result).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
	});
	it('refuses work reads and writes rather than writing sample work to storage', async () => {
		await expect(workApi.snapshot('org')).rejects.toMatchObject({ code: 'unavailable' });
		await expect(sender('org')({ method: 'task.create', item_id: '01ARZ3NDEKTSV4RRFFQ69G5FAV', project_id: 'project', title: 'Task', priority: 'normal' })).rejects.toMatchObject({ code: 'unavailable' });
		expect(workApi.requests).toBe(false);
	});
	it('keeps the missing-adapter error explicit through the existing browser error boundary', async () => {
		const error = await officeApi.connections().catch(toAppError);
		expect(error).toMatchObject({ source: 'api', code: 'unavailable', message: 'This area is not available yet.' });
	});
});
