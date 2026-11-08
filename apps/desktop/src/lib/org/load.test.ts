import { describe, expect, it, vi } from 'vitest';

const view = vi.fn();
vi.mock('./api', () => ({ orgApi: { view: (...args: unknown[]) => view(...args) } }));

import { loadOrgView } from './load';

describe('loadOrgView', () => {
	it('returns what the office answers', async () => {
		view.mockResolvedValueOnce({ snapshot: { members: [] }, sample: true });
		expect(await loadOrgView('org-1')).toEqual({ snapshot: { members: [] }, sample: true });
		expect(view).toHaveBeenCalledWith('org-1');
	});

	it('fails the page with the office’s own message, not a generic one', async () => {
		view.mockRejectedValueOnce({ source: 'api', code: 'not_found', message: "That organization isn't here." });
		await expect(loadOrgView('gone')).rejects.toMatchObject({ status: 500, body: { message: "That organization isn't here." } });
	});

	it('also explains a failure that is not from the office', async () => {
		view.mockRejectedValueOnce(new Error('boom'));
		await expect(loadOrgView('x')).rejects.toMatchObject({ status: 500, body: { message: 'boom' } });
	});
});
