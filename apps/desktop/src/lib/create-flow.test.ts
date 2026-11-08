import { describe, expect, it } from 'vitest';
import { createFlow, createOrganization, describeFailure, isBusy, type Phase } from './create-flow';
import { createMockOffice, type Scenario } from './office/mock';
import { toAppError, type OfficeApi } from './office/api';

// Each mock gets its own in-memory storage, so tests don't share remembered connections.
const memory = () => {
	const m = new Map<string, string>();
	return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};
const mock = (scenario: Scenario = 'healthy') => createMockOffice({ delayMs: 0, scenario, storage: memory() });
const run = async (api: OfficeApi, name: string) => {
	const phases: Phase[] = [];
	await createOrganization(api, name, (p) => phases.push(p));
	return phases;
};
const kinds = (phases: Phase[]) => phases.map((p) => p.kind);

describe('isBusy', () => {
	it('is true while working and after success (the form is navigating away), false otherwise', () => {
		const failed: Phase = { kind: 'failed', during: 'starting', error: { source: 'app', code: 'x', message: 'm' } };
		expect(['starting', 'creating'].map((kind) => isBusy({ kind } as Phase))).toEqual([true, true]);
		expect(isBusy({ kind: 'done', connection: {} as never })).toBe(true);
		expect(isBusy({ kind: 'editing' })).toBe(false);
		expect(isBusy(failed)).toBe(false);
	});
});

describe('createOrganization', () => {
	it('walks starting → creating → done for a good name', async () => {
		const phases = await run(mock(), '  Acme  ');
		expect(kinds(phases)).toEqual(['starting', 'creating', 'done']);
		const done = phases.at(-1);
		expect(done?.kind === 'done' && done.connection.organization.name).toBe('Acme');
	});

	it('a second organization with the same name is sent back to the form', async () => {
		const api = mock();
		await run(api, 'Acme');
		const phases = await run(api, 'aCME');
		expect(phases.at(-1)).toEqual({ kind: 'editing', nameError: 'An organization with that name already exists.' });
		expect((await api.connections()).length).toBe(1);
	});

	it('answers a bad name without calling the office', async () => {
		let called = false;
		const api: OfficeApi = {
			...mock(),
			connections: async () => [],
			ping: async () => ({ api_version: 1, server_version: 't', host: 'This Mac' }),
			restartOffice: async () => {},
			listOrganizations: async () => [],
			openOrganization: async () => Promise.reject(new Error('unused')),
			ensureRunning: async () => ((called = true), Promise.reject(new Error('unreachable'))),
			createOrganization: async () => Promise.reject(new Error('unreachable'))
		};
		const phases = await run(api, '   ');
		expect(phases).toEqual([{ kind: 'editing', nameError: 'Give the organization a name.' }]);
		expect(called).toBe(false);
	});

	it.each([
		['not_installed', 'starting'],
		['needs_approval', 'starting'],
		['unreachable', 'starting'],
		['storage_error', 'creating']
	] as const)('%s fails during %s', async (scenario, during) => {
		const phases = await run(mock(scenario), 'Acme');
		const last = phases.at(-1);
		expect(last?.kind).toBe('failed');
		expect(last?.kind === 'failed' && last.during).toBe(during);
		expect(last?.kind === 'failed' && last.error.code).toBe(scenario);
	});

	it('sends a name the office rejects back to the form', async () => {
		const api: OfficeApi = {
			...mock(),
			connections: async () => [],
			ping: async () => ({ api_version: 1, server_version: 't', host: 'This Mac' }),
			restartOffice: async () => {},
			listOrganizations: async () => [],
			openOrganization: async () => Promise.reject(new Error('unused')),
			ensureRunning: async () => ({
				api_version: 1,
				server_version: 't',
				host: 'This Mac'
			}),
			createOrganization: async () =>
				Promise.reject({ source: 'api', code: 'already_exists', message: 'Taken.' })
		};
		const phases = await run(api, 'Acme');
		expect(phases.at(-1)).toEqual({ kind: 'editing', nameError: 'Taken.' });
	});

	it('turns an unexpected throw into a failed phase', async () => {
		const api: OfficeApi = {
			...mock(),
			connections: async () => [],
			ping: async () => ({ api_version: 1, server_version: 't', host: 'This Mac' }),
			restartOffice: async () => {},
			listOrganizations: async () => [],
			openOrganization: async () => Promise.reject(new Error('unused')),
			ensureRunning: async () => Promise.reject(new Error('boom')),
			createOrganization: async () => Promise.reject(new Error('unused'))
		};
		const last = (await run(api, 'Acme')).at(-1);
		expect(last).toMatchObject({ kind: 'failed', during: 'starting', error: { source: 'app', message: 'boom' } });
	});
});

describe('describeFailure', () => {
	it('words a storage error for what the owner was doing', () => {
		const error = { source: 'api', code: 'storage_error', message: 'm' } as const;
		expect(describeFailure(error).title).toBe("Couldn't create the organization");
		expect(describeFailure(error, 'list').title).toBe("Couldn't read the organizations");
	});
});

describe('the office host login service', () => {
	it('approval offers to open Login Items; other failures do not', () => {
		const setup = (code: 'needs_approval' | 'not_installed') => describeFailure({ source: 'setup', code, message: 'm' });
		expect(setup('needs_approval').action).toBe('open_login_items');
		expect(describeFailure({ source: 'setup', code: 'turned_off', message: 'm' })).toMatchObject({ title: 'The office is off', action: 'turn_on' });
		expect(setup('not_installed').action).toBeUndefined();
	});

	it('the mock reports the state macOS would, and turning it on or off moves it', async () => {
		expect(await mock().loginItem()).toBe('enabled');
		expect(await mock('needs_approval').loginItem()).toBe('requires_approval');
		expect(await mock('not_installed').loginItem()).toBe('not_found');
		const api = mock();
		expect(await api.setLoginItem(false)).toBe('not_registered');
		expect(await api.setLoginItem(true)).toBe('enabled');
		// Waiting for approval stays waiting; a build without the service can't be turned on.
		expect(await mock('needs_approval').setLoginItem(true)).toBe('requires_approval');
		await expect(mock('not_installed').setLoginItem(true)).rejects.toMatchObject({ code: 'login_item' });
	});
});

describe('toAppError', () => {
	it('keeps known codes and maps unrecognised api codes to unknown', () => {
		expect(toAppError({ source: 'api', code: 'already_exists', message: 'm' }).code).toBe('already_exists');
		expect(toAppError({ source: 'api', code: 'from_the_future', message: 'm' }).code).toBe('unknown');
		expect(toAppError({ source: 'setup', code: 'not_installed', message: 'm' }).code).toBe('not_installed');
		expect(toAppError(new Error('boom'))).toEqual({ source: 'app', code: 'internal', message: 'boom' });
	});
});

describe('createFlow', () => {
	it('ignores a second submit while the first is running', async () => {
		let starts = 0;
		const inner = mock();
		const api: OfficeApi = {
			...inner,
			ensureRunning: () => (starts++, inner.ensureRunning())
		};
		const phases: Phase[] = [];
		const flow = createFlow(api, (p) => phases.push(p));
		await Promise.all([flow.submit('Acme'), flow.submit('Acme')]);
		expect(starts).toBe(1);
		expect(kinds(phases)).toEqual(['starting', 'creating', 'done']);
	});

	it('accepts a new submit once the previous run has finished (for example after a failure)', async () => {
		const flow = createFlow(mock('unreachable'), () => {});
		await flow.submit('Acme');
		let second = 0;
		const retry = createFlow({ ...mock(), ensureRunning: async () => (second++, Promise.reject(new Error('x'))) }, () => {});
		await retry.submit('Acme');
		await retry.submit('Acme');
		expect(second).toBe(2);
	});
});
