import { isHttpError } from '@sveltejs/kit';
import { describe, expect, it } from 'vitest';
import { orgApi } from '$lib/org/api';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { loadDirectory } from './load';

describe('loadDirectory', () => {
	it('answers no snapshot, without failing the page, while the production API is unavailable', async () => {
		await expect(loadDirectory('org-1', unavailableCall)).resolves.toMatchObject({
			snapshot: null
		});
	});

	it('the production org API is that unavailable adapter, so the route renders the directory', async () => {
		await expect(loadDirectory('org-1', (id) => orgApi.view(id))).resolves.toMatchObject({
			snapshot: null
		});
	});

	it('passes the snapshot through when the office serves it', async () => {
		const snapshot = { position: { seq: 1 }, members: [], teams: [] };
		const read = async () => ({ snapshot }) as unknown as Awaited<ReturnType<typeof orgApi.view>>;
		await expect(loadDirectory('org-1', read)).resolves.toMatchObject({ snapshot });
	});

	it('numbers every load, so the directory re-reads its rows after an org change elsewhere', async () => {
		const first = await loadDirectory('org-1', unavailableCall);
		const second = await loadDirectory('org-1', unavailableCall);
		expect(second.loads).toBeGreaterThan(first.loads);
	});

	it('fails the page with the real message for any other failure', async () => {
		const read = async () => {
			throw { source: 'api', code: 'not_found', message: 'That organization is gone.' };
		};
		const failure = await loadDirectory('org-1', read).catch((e) => e);
		expect(isHttpError(failure)).toBe(true);
		expect(failure.status).toBe(404);
		expect(failure.body.message).toBe('That organization is gone.');
	});
});
