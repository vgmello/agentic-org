import { describe, expect, it } from 'vitest';
import { createMockOrg, type MockStorage } from './api';

const memory = (): MockStorage => {
	const m = new Map<string, string>();
	return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};
const make = (storage = memory()) => createMockOrg(async () => 'office-1', { storage });

describe('the mock office’s organization', () => {
	it('starts with the sample, applies a command, and keeps the change across instances', async () => {
		const storage = memory();
		const first = make(storage);
		const accepted = await first.command('org-1', { method: 'team.create', params: { team_id: 't-new', name: 'Ops' } });
		expect(accepted.status).toBe('committed');
		expect((await make(storage).view('org-1')).snapshot.teams.map((t) => t.name)).toContain('Ops');
	});

	it('answers a repeated command id the same way and changes nothing again', async () => {
		const org = make();
		const command = { method: 'team.create', params: { command_id: 'C1', team_id: 't-new', name: 'Ops' } } as const;
		const a = await org.command('org-1', command);
		const b = await org.command('org-1', command);
		expect(b).toEqual(a);
		expect((await org.view('org-1')).snapshot.teams.filter((t) => t.name === 'Ops')).toHaveLength(1);
		expect((await org.view('org-1')).snapshot.position.seq).toBe(a.position!.seq);
	});

	it('rejects with the error HQ would give, and changes nothing', async () => {
		const org = make();
		const before = await org.view('org-1');
		await expect(org.command('org-1', { method: 'team.create', params: { team_id: 'x', name: 'design' } })).rejects.toMatchObject({ source: 'api', code: 'already_exists' });
		expect(await org.view('org-1')).toEqual(before);
	});

	it('a different organization has its own state', async () => {
		const storage = memory();
		const a = createMockOrg(async () => 'o', { storage });
		const b = createMockOrg(async () => 'o', { storage });
		await a.command('a', { method: 'team.create', params: { team_id: 'x', name: 'Only in A' } });
		expect((await b.view('b')).snapshot.teams.map((t) => t.name)).not.toContain('Only in A');
	});
});

describe('the mock office’s command handling, like HQ’s', () => {
	it('remembers a refusal: a retry gets the same refusal, and the position never moved', async () => {
		const org = make();
		const bad = { method: 'team.create', params: { command_id: 'R1', team_id: 'x', name: 'design' } } as const;
		const first = await org.command('org-1', bad).catch((e) => e);
		const again = await org.command('org-1', bad).catch((e) => e);
		expect(again).toEqual(first);
		expect(first).toMatchObject({ code: 'already_exists' });
		expect((await org.view('org-1')).snapshot.position.seq).toBe(1);
	});

	it('refuses a command id reused for a different command', async () => {
		const org = make();
		await org.command('org-1', { method: 'team.create', params: { command_id: 'S1', team_id: 'a', name: 'A' } });
		await expect(org.command('org-1', { method: 'team.create', params: { command_id: 'S1', team_id: 'b', name: 'B' } })).rejects.toMatchObject({ code: 'invalid_request' });
		expect((await org.view('org-1')).snapshot.teams.some((t) => t.id === 'b')).toBe(false);
	});

	it('answers commands sent together one at a time, so none is lost', async () => {
		const storage = memory();
		const org = make(storage);
		await Promise.all(['a', 'b', 'c'].map((id) => org.command('org-1', { method: 'team.create', params: { team_id: id, name: id.toUpperCase() } })));
		const teams = (await make(storage).view('org-1')).snapshot.teams.map((t) => t.id);
		expect(teams).toEqual(expect.arrayContaining(['a', 'b', 'c']));
		expect((await org.view('org-1')).snapshot.position.seq).toBe(4);
	});

	it('ignores stored data that is not an organization, and a command id like "constructor" is just an id', async () => {
		const storage = memory();
		storage.setItem('agentic-org.mock.org.org-1', '{}');
		const org = make(storage);
		expect((await org.view('org-1')).snapshot.teams.length).toBeGreaterThan(0);
		await expect(org.command('org-1', { method: 'team.create', params: { command_id: 'constructor', team_id: 'z', name: 'Z' } })).resolves.toMatchObject({ status: 'committed' });
	});

	it('says so when the change can’t be saved', async () => {
		const full: MockStorage = { getItem: () => null, setItem: () => { throw new Error('quota'); } };
		await expect(make(full).command('org-1', { method: 'team.create', params: { team_id: 'a', name: 'A' } })).rejects.toMatchObject({ source: 'app', code: 'internal' });
	});
});

describe('the browser mock office’s organization view', () => {
	it('is always sample data, and places agents in the office it was given', async () => {
		const view = await createMockOrg(async () => 'office-7', { storage: memory() }).view('org-1');
		expect(view.sample).toBe(true);
		expect(view.snapshot.members.filter((m) => m.kind === 'agent' && m.office_id === 'office-7').length).toBeGreaterThan(0);
	});

	it('shows only the owner when asked for an empty organization', async () => {
		const view = await createMockOrg(async () => 'o', { empty: () => true, storage: memory() }).view('org-1');
		expect(view.snapshot.members.map((m) => m.owner)).toEqual([true]);
	});

	it('refuses an organization that is not here even when it would show an empty one', async () => {
		const org = createMockOrg(async () => {
			throw { source: 'api', code: 'not_found', message: 'gone' };
		}, () => true);
		await expect(org.view('nope')).rejects.toMatchObject({ code: 'not_found' });
	});

	it('asks which organization it is for, and passes a refusal on', async () => {
		const asked: string[] = [];
		const org = createMockOrg(
			async (id) => {
				asked.push(id);
				throw { source: 'api', code: 'not_found', message: 'gone' };
			},
			{ storage: memory() }
		);
		await expect(org.view('org-9')).rejects.toMatchObject({ code: 'not_found' });
		expect(asked).toEqual(['org-9']);
	});
});
