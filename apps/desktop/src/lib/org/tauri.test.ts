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
