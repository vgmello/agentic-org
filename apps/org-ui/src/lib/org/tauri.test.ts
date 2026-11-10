import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { tauriOrg } from './tauri';

const snapshot = (kind: string) => ({
	position: { epoch: 1, seq: 1 },
	teams: [],
	members: [{ id: 'm', kind, name: 'M', version: 1 }],
	memberships: []
});

describe('tauriOrg.view', () => {
	beforeEach(() => invoke.mockReset());

	it('asks the shell for the organization with the camelCase key Tauri maps to organization_id', async () => {
		invoke.mockResolvedValueOnce({ snapshot: snapshot('person'), sample: true });
		await tauriOrg.view('org-1');
		expect(invoke).toHaveBeenCalledWith('org_snapshot', { organizationId: 'org-1' });
	});

	it('keeps the sample flag and reads a kind this build does not know as unknown', async () => {
		invoke.mockResolvedValueOnce({ snapshot: snapshot('robot'), sample: false });
		const view = await tauriOrg.view('org-1');
		expect(view.sample).toBe(false);
		expect(view.snapshot.members[0].kind).toBe('unknown');
	});

	it('lets the shell’s error through for the page to show', async () => {
		invoke.mockRejectedValueOnce({ source: 'api', code: 'storage_error', message: 'unreadable' });
		await expect(tauriOrg.view('org-1')).rejects.toMatchObject({ code: 'storage_error' });
	});
});

describe('tauriOrg.command', () => {
	beforeEach(() => invoke.mockReset());

	it('sends the command with its own id, or gives it one, to org_command', async () => {
		invoke.mockResolvedValue({ status: 'committed', position: { epoch: 1, seq: 2 } });
		await tauriOrg.command('org-1', { method: 'team.create', params: { command_id: 'C1', team_id: 't', name: 'Ops' } });
		expect(invoke).toHaveBeenLastCalledWith('org_command', {
			organizationId: 'org-1',
			command: { method: 'team.create', params: { command_id: 'C1', team_id: 't', name: 'Ops' } }
		});
		await tauriOrg.command('org-1', { method: 'team.delete', params: { team_id: 't', expected_version: 1 } });
		const sent = invoke.mock.calls.at(-1)![1].command;
		expect(sent.params.command_id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
	});

	it('lets HQ’s refusal through for the dialog to show', async () => {
		invoke.mockRejectedValueOnce({ source: 'api', code: 'already_exists', message: 'Taken.' });
		await expect(tauriOrg.command('org-1', { method: 'team.create', params: { team_id: 't', name: 'Ops' } })).rejects.toMatchObject({ code: 'already_exists' });
	});
});

describe('tauriOrg.command keeps the order commands were sent in', () => {
	beforeEach(() => invoke.mockReset());

	it('starts the next command only after the previous one is answered', async () => {
		const order: string[] = [];
		let release!: () => void;
		invoke.mockImplementationOnce(() => new Promise((resolve) => (release = () => (order.push('first answered'), resolve({ status: 'committed' })))));
		invoke.mockImplementationOnce(async () => (order.push('second sent'), { status: 'committed' }));
		const a = tauriOrg.command('o', { method: 'team.create', params: { team_id: 'a', name: 'A' } });
		const b = tauriOrg.command('o', { method: 'team.delete', params: { team_id: 'a', expected_version: 1 } });
		await Promise.resolve();
		expect(invoke).toHaveBeenCalledTimes(1); // the second waits
		release();
		await Promise.all([a, b]);
		expect(order).toEqual(['first answered', 'second sent']);
	});

	it('a refused command doesn’t stop the ones after it, and null means not given', async () => {
		invoke.mockRejectedValueOnce({ source: 'api', code: 'not_found', message: 'gone' });
		invoke.mockResolvedValueOnce({ status: 'committed' });
		const a = tauriOrg.command('o', { method: 'team.set_lead', params: { team_id: 'x', expected_version: 1, lead_member_id: null as unknown as undefined } });
		await expect(a).rejects.toMatchObject({ code: 'not_found' });
		await tauriOrg.command('o', { method: 'team.set_lead', params: { team_id: 'y', expected_version: 1, lead_member_id: null as unknown as undefined } });
		const sent = invoke.mock.calls[1][1].command.params;
		expect(sent).not.toHaveProperty('lead_member_id');
	});
});
