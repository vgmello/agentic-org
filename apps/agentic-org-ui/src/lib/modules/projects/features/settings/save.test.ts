import { describe, expect, it, vi } from 'vitest';
import { SectionSave } from './save.svelte';

describe('SectionSave controller', () => {
	it('marks saved after change and reload succeed, then clears on edited()', async () => {
		const reload = vi.fn(async () => {});
		const save = new SectionSave(reload);

		const ok = await save.run(async () => {}, 'Atlas');
		expect(ok).toBe(true);
		expect(save.saved).toBe(true);
		expect(save.busy).toBe(false);
		expect(save.refusal).toBeUndefined();
		expect(reload).toHaveBeenCalledTimes(1);

		save.edited();
		expect(save.saved).toBe(false);
	});

	it('reloads and surfaces conflict copy on revision conflicts without reloading on field refusals', async () => {
		const reload = vi.fn(async () => {});
		const save = new SectionSave(reload);

		const fieldOk = await save.run(async () => {
			throw { source: 'api', code: 'invalid_argument', field: 'name', message: 'Too short' };
		}, 'Atlas');
		expect(fieldOk).toBe(false);
		expect(save.refusal).toEqual({ field: 'name', message: 'Too short' });
		expect(reload).not.toHaveBeenCalled();

		const conflictOk = await save.run(async () => {
			throw { source: 'api', code: 'conflict', message: 'Stale revision' };
		}, 'Atlas');
		expect(conflictOk).toBe(false);
		expect(save.refusal?.message).toContain('Atlas');
		expect(reload).toHaveBeenCalledTimes(1);
	});
});
