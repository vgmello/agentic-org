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
