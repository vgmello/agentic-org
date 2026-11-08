import { describe, expect, it } from 'vitest';
import type { OfficeWithExtras } from '$lib/proto';
import { createMockOffices, type MockStorage } from './api';

const memory = (): MockStorage => {
	const m = new Map<string, string>();
	return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};
const hq: OfficeWithExtras = { id: 'hq', organization_id: 'o', name: 'HQ', role: 'hq', host: 'This Mac', status: 'active' };
const make = (offices = [hq], opts = {}) => createMockOffices(async () => ({ id: 'o', offices }), { stepMs: 0, storage: memory(), ...opts });

describe('the mock offices', () => {
	it('says it is simulated', () => {
		expect(make().sample).toBe(true);
	});

	it('adds an office on top of what the host lists, and never replaces the host’s list', async () => {
		const storage = memory();
		const mk = (offices: OfficeWithExtras[]) => createMockOffices(async () => ({ id: 'o', offices }), { stepMs: 0, storage });
		await mk([hq]).addHere('Office 2');
		expect((await mk([hq]).list()).map((o) => o.name)).toEqual(['HQ', 'Office 2']);
		// The host's own list changes (HQ's status goes down): it shows, with the added office still there.
		const down = { ...hq, status: 'unavailable' as const };
		const listed = await mk([down]).list();
		expect(listed.find((o) => o.id === 'hq')?.status).toBe('unavailable');
		expect(listed.map((o) => o.name)).toEqual(['HQ', 'Office 2']);
	});

	it('a removal hides the office, and HQ can’t be removed', async () => {
		const api = make([hq, { ...hq, id: 'o2', name: 'Office 2', role: 'satellite' }]);
		await api.remove('o2');
		expect((await api.list()).map((o) => o.id)).toEqual(['hq']);
		await expect(api.remove('hq')).rejects.toMatchObject({ code: 'invalid_request' });
		await expect(api.remove('ghost')).rejects.toMatchObject({ code: 'not_found' });
	});

	it('refuses a duplicate name, including one that appeared while it was starting', async () => {
		const api = make();
		await expect(api.addHere('hq')).rejects.toMatchObject({ code: 'invalid_name' });
		await api.addHere('Office 2');
		await expect(api.addHere('office 2')).rejects.toMatchObject({ code: 'invalid_name' });
	});

	it('a failed start leaves a Stopped office with its reason, and restart tries it again', async () => {
		const failing = make([hq], { failStart: true });
		const error = await failing.addHere('Office 2').catch((e) => e);
		expect(error).toMatchObject({ code: 'unavailable', office_id: expect.any(String) });
		const stopped = (await failing.list()).find((o) => o.name === 'Office 2')!;
		expect(stopped).toMatchObject({ status: 'unavailable', stopped_reason: expect.any(String) });
		await expect(failing.restart(stopped.id)).rejects.toMatchObject({ code: 'unavailable' });
		await expect(failing.restart('ghost')).rejects.toMatchObject({ code: 'not_found' });
	});

	it('restart brings a stopped office online and clears its reason', async () => {
		const storage = memory();
		const mk = (failStart: boolean) => createMockOffices(async () => ({ id: 'o', offices: [hq] }), { stepMs: 0, storage, failStart });
		const err = await mk(true).addHere('Office 2').catch((e) => e);
		await mk(false).restart(err.office_id);
		const office = (await mk(false).list()).find((o) => o.name === 'Office 2')!;
		expect(office.status).toBe('active');
		expect(office.stopped_reason).toBeUndefined();
	});

	it('ignores unreadable stored changes', async () => {
		const storage = memory();
		storage.setItem('agentic-org.mock.offices.o', '{ nonsense');
		const api = createMockOffices(async () => ({ id: 'o', offices: [hq] }), { stepMs: 0, storage });
		expect((await api.list()).map((o) => o.id)).toEqual(['hq']);
		storage.setItem('agentic-org.mock.offices.o', JSON.stringify({ added: [null, { id: 3 }, { id: 'x', name: 'Kept' }], removed: [1, 'gone'] }));
		expect((await api.list()).map((o) => o.name)).toEqual(['HQ', 'Kept']);
	});
});
