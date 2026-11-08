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
	it('remembers a refusal even when the same command would now succeed', async () => {
		const org = make();
		const c = { method: 'team.create', params: { command_id: 'R1', team_id: 'ops', name: 'Ops', owner_team_id: 'later' } } as const;
		await expect(org.command('org-1', c)).rejects.toMatchObject({ code: 'not_found' });
		await org.command('org-1', { method: 'team.create', params: { team_id: 'later', name: 'Later' } });
		await expect(org.command('org-1', c)).rejects.toMatchObject({ code: 'not_found' }); // the answer stands
		expect((await org.view('org-1')).snapshot.teams.some((t) => t.id === 'ops')).toBe(false);
		await expect(org.command('org-1', { ...c, params: { ...c.params, command_id: 'R2' } })).resolves.toMatchObject({ status: 'committed' });
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
		}, { empty: () => true });
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

describe('the browser mock keeps its history bounded, like the Rust mock HQ', () => {
	it('keeps only the recent answers: the oldest command is judged afresh', async () => {
		const storage = memory();
		const org = make(storage);
		const first = { method: 'team.create', params: { command_id: 'F0', team_id: 't0', name: 'T0' } } as const;
		await org.command('org-1', first);
		for (let n = 1; n <= 1000; n++) await org.command('org-1', { method: 'team.create', params: { command_id: `F${n}`, team_id: `t${n}`, name: `T${n}` } });
		await expect(org.command('org-1', first)).rejects.toMatchObject({ code: 'already_exists' });
		expect(storage.getItem('agentic-org.mock.org.org-1')!.length).toBeLessThan(2_000_000);
	});

	it('refuses a command too big to be a command, and an unreadable id, without keeping them', async () => {
		const storage = memory();
		const org = make(storage);
		await expect(org.command('org-1', { method: 'team.create', params: { team_id: 'big', name: 'x'.repeat(20_000) } })).rejects.toMatchObject({ code: 'invalid_request' });
		await expect(org.command('org-1', { method: 'team.create', params: { command_id: '', team_id: 'z', name: 'Z' } })).rejects.toMatchObject({ code: 'invalid_request' });
		expect(storage.getItem('agentic-org.mock.org.org-1')).toBeNull();
	});

	it('an empty organization keeps its changes for the session, and nothing in storage', async () => {
		const storage = memory();
		const org = createMockOrg(async () => 'o', { empty: () => true, storage });
		await org.command('org-1', { method: 'team.create', params: { team_id: 'a', name: 'A' } });
		expect((await org.view('org-1')).snapshot.teams.map((t) => t.id)).toEqual(['a']);
		expect(storage.getItem('agentic-org.mock.org.org-1')).toBeNull();
	});

	it('ignores a stored answer that is not an answer instead of failing on it', async () => {
		const storage = memory();
		const first = make(storage);
		await first.view('org-1');
		const saved = { snapshot: (await first.view('org-1')).snapshot, answers: { X: { command: 'x', result: null } } };
		storage.setItem('agentic-org.mock.org.org-1', JSON.stringify(saved));
		await expect(make(storage).command('org-1', { method: 'team.create', params: { command_id: 'X', team_id: 'n', name: 'N' } })).resolves.toMatchObject({ status: 'committed' });
	});
});

describe('the browser mock agrees with the Rust mock on the edges', () => {
	it('counts the size of a command in bytes, so 6000 wide characters are too big', async () => {
		const org = make();
		await expect(org.command('org-1', { method: 'team.create', params: { team_id: 'w', name: '名'.repeat(6000) } })).rejects.toMatchObject({ code: 'invalid_request' });
		await expect(org.command('org-1', { method: 'team.create', params: { team_id: 'a', name: 'a'.repeat(60) } })).resolves.toMatchObject({ status: 'committed' });
	});

	it('answers not_found for a missing organization before it judges the request', async () => {
		const org = createMockOrg(async () => {
			throw { source: 'api', code: 'not_found', message: 'gone' };
		}, { storage: memory() });
		await expect(org.command('missing', { method: 'team.create', params: { command_id: '', team_id: 'a', name: 'A' } })).rejects.toMatchObject({ code: 'not_found' });
	});

	it('a stored list of answers that is not a list is ignored, and a bad answer is replaced once in the order', async () => {
		const storage = memory();
		const first = make(storage);
		const snapshot = (await first.view('org-1')).snapshot;
		storage.setItem('agentic-org.mock.org.org-1', JSON.stringify({ snapshot, answers: {}, order: 'x' }));
		await expect(make(storage).command('org-1', { method: 'team.create', params: { command_id: 'Q', team_id: 'q', name: 'Q' } })).resolves.toMatchObject({ status: 'committed' });
		storage.setItem('agentic-org.mock.org.org-1', JSON.stringify({ snapshot, answers: { Z: { command: 'x', result: null } }, order: ['Z'] }));
		await make(storage).command('org-1', { method: 'team.create', params: { command_id: 'Z', team_id: 'z', name: 'Z' } });
		expect(JSON.parse(storage.getItem('agentic-org.mock.org.org-1')!).order).toEqual(['Z']);
	});
});

describe('null means not given, as on the wire', () => {
	it('clears a lead with null instead of throwing, and an owner of null is a top-level team', async () => {
		const org = make();
		await org.command('org-1', { method: 'team.set_lead', params: { team_id: 'web', expected_version: 1, lead_member_id: null as unknown as undefined } });
		expect((await org.view('org-1')).snapshot.teams.find((t) => t.id === 'web')?.lead_member_id).toBeUndefined();
		await org.command('org-1', { method: 'team.create', params: { team_id: 'top', name: 'Top', owner_team_id: null as unknown as undefined } });
		expect((await org.view('org-1')).snapshot.teams.find((t) => t.id === 'top')?.owner_team_id).toBeUndefined();
	});
});
