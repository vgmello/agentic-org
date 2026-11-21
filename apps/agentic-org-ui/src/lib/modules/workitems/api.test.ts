import { describe, expect, it } from 'vitest';
import { workItemsApi } from './api';

describe('workItemsApi production adapter', () => {
	it('refuses every read and write with unavailable (zero static mocks in src)', async () => {
		for (const [name, fn] of Object.entries(workItemsApi)) {
			expect(typeof fn, name).toBe('function');
			await expect((fn as (...args: unknown[]) => Promise<unknown>)(), name).rejects.toMatchObject({
				source: 'api',
				code: 'unavailable'
			});
		}
	});
});
