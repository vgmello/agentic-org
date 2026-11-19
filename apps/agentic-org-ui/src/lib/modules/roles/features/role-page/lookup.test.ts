import { expect, it } from 'vitest';
import type { RoleDetail } from '../../types';
import { lookUpRole } from './lookup';

const detail = { role: { id: 'r' } } as RoleDetail;

it('answers the role when it exists', async () => {
	expect(await lookUpRole(async () => detail)).toEqual({ found: true, detail });
});

it('turns not_found into a page state with the reason', async () => {
	const missing = { source: 'api', code: 'not_found', message: "There's no role with this ID." };
	expect(
		await lookUpRole(async () => {
			throw missing;
		})
	).toEqual({ found: false, message: "There's no role with this ID." });
});

it('lets every other failure through, for the error and unavailable states', async () => {
	const down = { source: 'api', code: 'unavailable', message: 'x' };
	await expect(
		lookUpRole(async () => {
			throw down;
		})
	).rejects.toBe(down);
});
