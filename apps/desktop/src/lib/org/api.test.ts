import { describe, expect, it } from 'vitest';
import { createMockOrg } from './api';

describe('the browser mock office’s organization', () => {
	it('is always sample data, and places agents in the office it was given', async () => {
		const view = await createMockOrg(async () => 'office-7').view('org-1');
		expect(view.sample).toBe(true);
		expect(view.snapshot.members.filter((m) => m.kind === 'agent' && m.office_id === 'office-7').length).toBeGreaterThan(0);
	});

	it('shows only the owner when asked for an empty organization', async () => {
		const view = await createMockOrg(async () => 'o', () => true).view('org-1');
		expect(view.snapshot.members.map((m) => m.owner)).toEqual([true]);
	});

	it('asks which organization it is for, and passes a refusal on', async () => {
		const asked: string[] = [];
		const org = createMockOrg(async (id) => {
			asked.push(id);
			throw { source: 'api', code: 'not_found', message: 'gone' };
		});
		await expect(org.view('org-9')).rejects.toMatchObject({ code: 'not_found' });
		expect(asked).toEqual(['org-9']);
	});
});
