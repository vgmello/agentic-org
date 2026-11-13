import { describe, expect, it, vi } from 'vitest';
import { createSessionController, type SessionState } from './session-controller';
import { SessionRequestError } from './session-gateway';
import type { SessionRecord } from './session-record';
import type { JsonHttpRequest } from '../http/json-http';

const proof = 'a'.repeat(64), rotated = 'b'.repeat(64);
const record: SessionRecord = { instance_id: 'hq', account_id: 'alex-id', login: 'alex', proof, rotated_at_ms: 100 };
const view = { instance_id: 'hq', account: { id: 'alex-id', login: 'alex' }, admin: false, organizations: [],
	server_time_ms: 100, absolute_expires_at: 'later', idle_expires_at: 'sooner' };
const unauthorized = () => new SessionRequestError(401, { code: 'not_authorized', message: proof, session_reason: 'hq_restarted' });
function deferred<T>() {
	let resolve!: (value: T) => void, reject!: (error: unknown) => void;
	const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
	return { promise, resolve, reject };
}
function setup(initial: SessionRecord | undefined = record) {
	let stored: SessionRecord | undefined = initial;
	const store = {
		read: vi.fn(async () => stored), write: vi.fn(async (value: SessionRecord) => { stored = value; }),
		clear: vi.fn(async () => { stored = undefined; })
	};
	const gateway = {
		version: vi.fn(async () => ({ api_version: 1, contract_revision: 2 })), get: vi.fn(async (_proof: string) => view),
		signIn: vi.fn(async (_credentials: unknown, _signal?: AbortSignal) => ({ ...view, proof: rotated })),
		refresh: vi.fn(async (_proof: string, _signal?: AbortSignal) => ({ ...view, proof: rotated })),
		signOut: vi.fn(async (_proof: string, _signal?: AbortSignal) => ({}))
	};
	const listeners = new Set<EventListenerOrEventListenerObject>();
	const postMessage = vi.fn();
	const states: SessionState[] = [];
	let tail = Promise.resolve<unknown>(undefined);
	const names: string[] = [];
	const locks = { request: ((name: string, callback: () => unknown) => {
		names.push(name);
		const run = tail.then(callback);
		tail = run.catch(() => {});
		return run;
	}) as LockManager['request'] };
	const http = vi.fn<(request: JsonHttpRequest) => Promise<{ status: number; body: unknown }>>(async () => ({ status: 200, body: { result: {} } }));
	const controller = createSessionController({ store, gateway, http, locks, now: () => 200,
		channel: { postMessage,
			addEventListener: ((_type: string, listener: EventListenerOrEventListenerObject) => { listeners.add(listener); }) as BroadcastChannel['addEventListener'],
			removeEventListener: ((_type: string, listener: EventListenerOrEventListenerObject) => { listeners.delete(listener); }) as BroadcastChannel['removeEventListener'] },
		changed: state => states.push(state) });
	return { controller, store, gateway, http, states, postMessage, names, stored: () => stored, listenerCount: () => listeners.size,
		replace: (value: SessionRecord | undefined) => { stored = value; },
		broadcast: (data: unknown) => { const event = new MessageEvent('message', { data }); for (const listener of listeners) {
			if (typeof listener === 'function') listener(event); else listener.handleEvent(event);
		} } };
}
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe('P6a session ownership', () => {
	it('boots with negotiated revision and pair, publishing a proof-free view', async () => {
		const s = setup(); await s.controller.boot();
		expect(s.gateway.get).toHaveBeenCalledWith(proof);
		expect(s.states).toEqual([{ kind: 'signed-in', session: view }]);
		expect(JSON.stringify(s.states)).not.toContain(proof);
	});
	it.each([{ api_version: 2, contract_revision: 2 }, { api_version: 1, contract_revision: 1 }])('refuses incompatible version %j before credential recovery', async version => {
		const s = setup(); s.gateway.version.mockResolvedValue(version); await s.controller.boot();
		expect(s.states).toEqual([{ kind: 'update-required' }]);
		expect(s.store.read).not.toHaveBeenCalled(); expect(s.gateway.get).not.toHaveBeenCalled();
	});
	it('has no cookie-only boot recovery', async () => {
		const s = setup(); s.replace(undefined); await s.controller.boot();
		expect(s.gateway.get).not.toHaveBeenCalled(); expect(s.states.at(-1)).toMatchObject({ kind: 'signed-out', reason: 'ended' });
	});
	it.each(['instance', 'account', '401'] as const)('ends boot on %s without a mounted-state dialog or deleting the record', async mismatch => {
		const s = setup();
		if (mismatch === '401') s.gateway.get.mockRejectedValue(unauthorized());
		else s.gateway.get.mockResolvedValue({ ...view, ...(mismatch === 'instance' ? { instance_id: 'other' } : { account: { id: 'other', login: 'other' } }) });
		await s.controller.boot(); expect(s.states.at(-1)).toMatchObject({ kind: 'signed-out', reason: 'ended' });
		expect(s.store.clear).not.toHaveBeenCalled(); expect(s.stored()).toEqual(record);
	});
	it('propagates transient boot errors rather than deleting credentials', async () => {
		const s = setup(); s.gateway.get.mockRejectedValue(new Error('offline'));
		await expect(s.controller.boot()).rejects.toThrow('offline'); expect(s.stored()).toEqual(record);
	});
	it('does not let a pending boot resurrect state after sign-out', async () => {
		const s = setup(); await s.controller.boot();
		const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		const pending = s.controller.boot(); await tick(); await s.controller.signOut(); old.resolve(view); await pending;
		expect(s.states.at(-1)).toMatchObject({ kind: 'signed-out', reason: 'signed-out' });
	});
	it('serializes sign-in, refresh and sign-out under one lock; persists before broadcasting', async () => {
		const s = setup(); await s.controller.boot();
		const first = deferred<{ [K in keyof typeof view]: typeof view[K] } & { proof: string }>();
		s.gateway.signIn.mockReturnValueOnce(first.promise);
		const login = s.controller.signIn({ login: 'alex', password: 'private' });
		const refresh = s.controller.refresh(200); const logout = s.controller.signOut();
		await tick(); expect(s.gateway.refresh).not.toHaveBeenCalled(); expect(s.gateway.signOut).not.toHaveBeenCalled();
		first.resolve({ ...view, proof: rotated }); await Promise.all([login, refresh, logout]);
		expect(s.names).toEqual(Array(3).fill('org-session:refresh'));
		expect(s.gateway.refresh).toHaveBeenCalledWith(rotated, undefined);
		expect(s.gateway.signOut).toHaveBeenCalledWith(rotated, undefined);
		expect(s.postMessage.mock.calls).toEqual([[{ kind: 'signed-in', account_id: 'alex-id' }], [{ kind: 'rotated' }], [{ kind: 'signed-out' }]]);
		expect(s.store.write.mock.invocationCallOrder[0]).toBeLessThan(s.postMessage.mock.invocationCallOrder[0]);
		expect(s.store.clear.mock.invocationCallOrder[0]).toBeLessThan(s.postMessage.mock.invocationCallOrder[2]);
		expect(JSON.stringify(s.postMessage.mock.calls)).not.toContain(proof); expect(s.stored()).toBeUndefined();
	});
	it('never publishes or broadcasts a sign-in before its storage transaction succeeds', async () => {
		const s = setup(); const write = deferred<void>(); s.store.write.mockReturnValueOnce(write.promise);
		const login = s.controller.signIn({ login: 'alex', password: 'private' }); await tick();
		expect(s.states).toEqual([]); expect(s.postMessage).not.toHaveBeenCalled();
		write.reject(new Error('storage')); await expect(login).rejects.toThrow('storage');
		expect(s.states).toEqual([]); expect(s.postMessage).not.toHaveBeenCalled();
	});
	it.each(['missing', 'account', 'instance', 'newer'] as const)('refresh re-reads under lock and skips a %s record', async change => {
		const s = setup(); await s.controller.boot();
		s.replace(change === 'missing' ? undefined : { ...record, ...(change === 'account' ? { account_id: 'other' } : change === 'instance' ? { instance_id: 'other' } : { rotated_at_ms: 101, proof: rotated }) });
		await s.controller.refresh(100);
		expect(s.gateway.refresh).not.toHaveBeenCalled(); expect(s.store.write).not.toHaveBeenCalled();
	});
	it.each([-1, NaN, Infinity, 0.5])('refuses an invalid explicit refresh trigger %j', async time => {
		const s = setup(); await s.controller.boot(); await expect(s.controller.refresh(time)).rejects.toThrow();
		expect(s.gateway.refresh).not.toHaveBeenCalled();
	});
	it.each(['instance', 'account'] as const)('never overwrites the record when refresh returns a mismatched %s', async mismatch => {
		const s = setup(); await s.controller.boot();
		s.gateway.refresh.mockResolvedValue({ ...view, proof: rotated, ...(mismatch === 'instance' ? { instance_id: 'other' } : { account: { id: 'other', login: 'other' } }) });
		await s.controller.refresh(100); expect(s.store.write).not.toHaveBeenCalled(); expect(s.stored()).toEqual(record);
		expect(s.states.at(-1)).toMatchObject({ kind: 'signed-out', reason: 'ended' });
	});
	it('preserves the mounted view and record on refresh 401, signaling reauthentication only', async () => {
		const s = setup(); await s.controller.boot(); s.gateway.refresh.mockRejectedValue(unauthorized());
		await expect(s.controller.refresh(100)).rejects.toMatchObject({ status: 401 });
		expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view, reauth: 'hq_restarted' }); expect(s.stored()).toEqual(record);
		expect(s.gateway.refresh).toHaveBeenCalledTimes(1);
	});
	it('keeps session and storage unchanged if the HQ cannot sign out', async () => {
		const s = setup(); await s.controller.boot(); s.gateway.signOut.mockRejectedValue(new Error('offline'));
		await expect(s.controller.signOut()).rejects.toThrow('offline');
		expect(s.states).toEqual([{ kind: 'signed-in', session: view }]); expect(s.stored()).toEqual(record);
		expect(s.postMessage).not.toHaveBeenCalled(); expect(s.store.clear).not.toHaveBeenCalled(); expect(s.gateway.signOut).toHaveBeenCalledTimes(1);
	});
	it('does not sign out an account that replaced the current one while waiting for the lock', async () => {
		const s = setup(); await s.controller.boot(); s.replace({ ...record, account_id: 'other' });
		await s.controller.signOut(); expect(s.gateway.signOut).not.toHaveBeenCalled(); expect(s.store.clear).not.toHaveBeenCalled();
	});
	it('re-reads the record on a broadcast and closes same-account reauth with the new proof', async () => {
		const s = setup(); await s.controller.boot(); s.http.mockResolvedValueOnce({ status: 401, body: {} }); await s.controller.read('/api/v1/capabilities');
		expect(s.states.at(-1)).toHaveProperty('reauth'); s.replace({ ...record, proof: rotated });
		s.broadcast({ kind: 'signed-in', account_id: record.account_id, proof: 'untrusted-message-proof' }); await tick();
		expect(s.gateway.get).toHaveBeenLastCalledWith(rotated); expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view });
	});
	it('clears old-account views and stays on the reload gate across later broadcasts', async () => {
		const s = setup(); await s.controller.boot(); s.replace({ ...record, account_id: 'other', login: 'Blair' });
		s.broadcast({ kind: 'signed-in', account_id: 'other' }); await tick();
		expect(s.states.at(-1)).toEqual({ kind: 'signed-out', reason: 'changed-account', login: 'Blair' });
		s.broadcast({ kind: 'rotated' }); await tick(); expect(s.states.at(-1)).toEqual({ kind: 'signed-out', reason: 'changed-account', login: 'Blair' });
		expect(s.gateway.get).toHaveBeenCalledTimes(1);
	});
	it('clears every tab when a signed-out broadcast finds no record', async () => {
		const s = setup(); await s.controller.boot(); s.replace(undefined); s.broadcast({ kind: 'signed-out' }); await tick();
		expect(s.states.at(-1)).toEqual({ kind: 'signed-out', reason: 'signed-out' });
	});
	it.each([null, 1, {}, { kind: {} }, { kind: 'other' }, { kind: 'signed-in' }])('ignores malformed notification %j', async value => {
		const s = setup(); s.broadcast(value); await tick(); expect(s.store.read).not.toHaveBeenCalled();
	});
	it('does not let an older broadcast recovery publish after a newer sign-in', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); s.gateway.signIn.mockResolvedValue({ ...view, account: { id: 'new', login: 'New' }, proof: rotated });
		await s.controller.signIn({ login: 'New', password: 'private' }); old.resolve(view); await tick();
		expect(s.states.at(-1)).toMatchObject({ kind: 'signed-in', session: { account: { id: 'new' } } });
	});
	it('retries exactly one READ after a proof change, preserving the path and signal', async () => {
		const s = setup(); await s.controller.boot(); s.replace({ ...record, proof: rotated });
		s.http.mockResolvedValueOnce({ status: 401, body: {} }).mockResolvedValueOnce({ status: 200, body: { result: {} } });
		const signal = new AbortController().signal; expect((await s.controller.read('/api/v1/capabilities', signal)).status).toBe(200);
		expect(s.http.mock.calls.map(([request]) => request)).toEqual([
			{ method: 'GET', path: '/api/v1/capabilities', proof, signal }, { method: 'GET', path: '/api/v1/capabilities', proof: rotated, signal }
		]); expect(s.states.at(-1)).not.toHaveProperty('reauth');
		await s.controller.read('/api/v1/capabilities', signal); expect(s.http.mock.calls[2][0].proof).toBe(rotated);
	});
	it('does not retry a 401 with the same proof, retaining the mounted view', async () => {
		const s = setup(); await s.controller.boot(); s.http.mockResolvedValue({ status: 401, body: {} });
		await s.controller.read('/api/v1/capabilities'); expect(s.http).toHaveBeenCalledTimes(1);
		expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view, reauth: 'session_ended' }); expect(s.stored()).toEqual(record);
	});
	it('does not make a third read when the retry also gets 401', async () => {
		const s = setup(); await s.controller.boot(); s.replace({ ...record, proof: rotated });
		s.http.mockResolvedValue({ status: 401, body: { error: { code: 'not_authorized', message: proof, session_reason: 'hq_restarted' } } } as never);
		await s.controller.read('/api/v1/capabilities'); expect(s.http).toHaveBeenCalledTimes(2);
		expect(s.states.at(-1)).toHaveProperty('reauth', 'hq_restarted');
	});
	it('does not retry a 403 permission refusal', async () => {
		const s = setup(); await s.controller.boot(); s.http.mockResolvedValue({ status: 403, body: {} });
		expect((await s.controller.read('/api/v1/capabilities')).status).toBe(403); expect(s.http).toHaveBeenCalledTimes(1);
		expect(s.states.at(-1)).not.toHaveProperty('reauth');
	});
	it.each(['missing', 'account', 'instance'] as const)('a 401 with %s storage clears old views without another read', async change => {
		const s = setup(); await s.controller.boot(); s.replace(change === 'missing' ? undefined : { ...record, ...(change === 'account' ? { account_id: 'other' } : { instance_id: 'other' }) });
		s.http.mockResolvedValueOnce({ status: 401, body: {} }); await expect(s.controller.read('/api/v1/capabilities')).rejects.toThrow('Session changed');
		expect(s.http).toHaveBeenCalledTimes(1); expect(s.states.at(-1)).toHaveProperty('kind', 'signed-out');
	});
	it('rejects a late response for the previous account after local sign-in', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: { result: {} } }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); s.gateway.signIn.mockResolvedValue({ ...view, account: { id: 'new', login: 'New' }, proof: rotated });
		await s.controller.signIn({ login: 'New', password: 'private' }); old.resolve({ status: 200, body: { result: {} } });
		await expect(read).rejects.toThrow('Session changed');
	});
	it('disposal detaches notifications and rejects late reads or new operations', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: { result: {} } }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); s.controller.dispose(); s.broadcast({ kind: 'signed-out' });
		old.resolve({ status: 200, body: { result: {} } }); await expect(read).rejects.toThrow('Session changed');
		await expect(s.controller.boot()).rejects.toThrow('closed'); await expect(s.controller.signIn({})).rejects.toThrow('closed');
		await expect(s.controller.refresh(100)).rejects.toThrow('closed'); await expect(s.controller.signOut()).rejects.toThrow('closed');
		await expect(s.controller.read('/api/v1/capabilities')).rejects.toThrow('closed'); expect(s.states).toHaveLength(1);
	});
	it('negotiates before sign-in even when boot has not run', async () => {
		const s = setup(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 });
		await expect(s.controller.signIn({ login: 'alex', password: 'private' })).rejects.toThrow('update required');
		expect(s.gateway.signIn).not.toHaveBeenCalled(); expect(s.store.write).not.toHaveBeenCalled();
		expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});
	it('an incompatible reboot invalidates already pending reads', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: { result: {} } }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 });
		await s.controller.boot(); old.resolve({ status: 200, body: { result: {} } }); await expect(read).rejects.toThrow('Session changed');
	});
	it('same-account sign-in does not discard a read that can recover the rotated proof', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: { result: {} } }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); await s.controller.signIn({ login: 'alex', password: 'private' });
		old.resolve({ status: 401, body: { result: {} } }); expect((await read).status).toBe(200);
		expect(s.http.mock.calls[1][0].proof).toBe(rotated);
	});
	it('unknown or malformed 401 reasons use safe reauthentication copy', async () => {
		for (const body of [null, { error: { code: 'not_authorized', message: proof, session_reason: 'future-reason' } }]) {
			const s = setup(); await s.controller.boot(); s.http.mockResolvedValue({ status: 401, body } as never);
			await s.controller.read('/api/v1/capabilities'); expect(s.states.at(-1)).toHaveProperty('reauth', body ? 'unknown' : 'session_ended');
		}
	});
	it.each(['instance', 'account', '401', 'offline'] as const)('broadcast recovery handles %s without publishing another account view', async reason => {
		const s = setup(); await s.controller.boot();
		if (reason === '401') s.gateway.get.mockRejectedValue(unauthorized());
		else if (reason === 'offline') s.gateway.get.mockRejectedValue(new Error('offline'));
		else s.gateway.get.mockResolvedValue({ ...view, ...(reason === 'instance' ? { instance_id: 'other' } : { account: { id: 'other', login: 'other' } }) });
		s.broadcast({ kind: 'rotated' }); await tick();
		if (reason === '401') expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view, reauth: 'hq_restarted' });
		else if (reason === 'offline') expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view });
		else expect(s.states.at(-1)).toEqual({ kind: 'signed-out', reason: 'ended' });
	});
	it('a later boot takes precedence over an older recovery attempt', async () => {
		const s = setup(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		const first = s.controller.boot(); await tick(); await s.controller.boot(); old.resolve({ ...view, account: { id: 'obsolete', login: 'obsolete' } });
		await first; expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view });
	});
	it('disposed sign-in may finish persisting its dispatched pair but cannot publish or notify', async () => {
		const s = setup(); const old = deferred<typeof view & { proof: string }>(); s.gateway.signIn.mockReturnValueOnce(old.promise);
		const login = s.controller.signIn({ login: 'alex', password: 'private' }); await tick(); s.controller.dispose();
		old.resolve({ ...view, proof: rotated }); await login;
		expect(s.stored()?.proof).toBe(rotated); expect(s.states).toEqual([]); expect(s.postMessage).not.toHaveBeenCalled();
	});
	it('a disposed queued operation never dispatches after it acquires the lock', async () => {
		const s = setup(); const old = deferred<typeof view & { proof: string }>(); s.gateway.signIn.mockReturnValueOnce(old.promise);
		const first = s.controller.signIn({ login: 'alex', password: 'private' }); await tick();
		const queued = s.controller.signIn({ login: 'Blair', password: 'private' }); s.controller.dispose(); old.resolve({ ...view, proof: rotated });
		await first; await expect(queued).rejects.toThrow('closed'); expect(s.gateway.signIn).toHaveBeenCalledTimes(1);
	});
	it('no signed-in record means no refresh, no sign-out and no read dispatch', async () => {
		const s = setup(); await s.controller.refresh(100); await s.controller.signOut();
		await expect(s.controller.read('/api/v1/capabilities')).rejects.toThrow('Sign in required');
		expect(s.names).toEqual([]); expect(s.http).not.toHaveBeenCalled(); expect(s.gateway.refresh).not.toHaveBeenCalled(); expect(s.gateway.signOut).not.toHaveBeenCalled();
	});

	it('sign-out completing after disposal cannot publish to a removed view', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{}>(); s.gateway.signOut.mockReturnValueOnce(old.promise);
		const logout = s.controller.signOut(); await tick(); s.controller.dispose(); old.resolve({}); await logout;
		expect(s.states).toHaveLength(1); expect(s.stored()).toBeUndefined(); expect(s.postMessage).not.toHaveBeenCalled();
	});
	it('an unauthenticated broadcast 401 cannot invent a mounted reauth state', async () => {
		const s = setup(); s.gateway.get.mockRejectedValue(unauthorized()); s.broadcast({ kind: 'rotated' }); await tick();
		expect(s.states).toEqual([]);
	});

	it('a signed-out tab accepts an authoritative same-origin sign-in broadcast', async () => {
		const s = setup(); s.broadcast({ kind: 'signed-in', account_id: 'alex-id' }); await tick();
		expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view });
	});
	it('sign-out invalidates an already pending read', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: { result: {} } }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); await s.controller.signOut(); old.resolve({ status: 200, body: { result: {} } });
		await expect(read).rejects.toThrow('Session changed');
	});
	it.each(['signIn', 'refresh'] as const)('a newer same-account %s invalidates an older boot view', async operation => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		const boot = s.controller.boot(); await tick();
		s.gateway[operation].mockResolvedValue({ ...view, admin: true, proof: rotated });
		if (operation === 'signIn') await s.controller.signIn({ login: 'alex', password: 'private' }); else await s.controller.refresh(200);
		old.resolve(view); await boot; expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: { ...view, admin: true } });
		expect(s.stored()?.rotated_at_ms).toBe(200);
	});
	it('the latest broadcast recovery wins even for the same account', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); s.gateway.get.mockResolvedValue({ ...view, admin: true });
		s.broadcast({ kind: 'rotated' }); await tick(); old.resolve(view); await tick();
		expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: { ...view, admin: true } });
	});
	it.each(['refresh', 'signOut'] as const)('a disposed queued %s cannot dispatch after acquiring its lock', async operation => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view & { proof: string }>(); s.gateway.signIn.mockReturnValueOnce(old.promise);
		const first = s.controller.signIn({ login: 'alex', password: 'private' }); await tick();
		const queued = operation === 'refresh' ? s.controller.refresh(200) : s.controller.signOut(); s.controller.dispose();
		old.resolve({ ...view, proof: rotated }); await first; await expect(queued).rejects.toThrow('closed');
		expect(s.gateway[operation]).not.toHaveBeenCalled();
	});

	it.each(['plain401', 'typed503'] as const)('boot distinguishes %s from an ended-session response', async mode => {
		const s = setup(); const error = mode === 'plain401' ? Object.assign(new Error('offline'), { status: 401 })
			: new SessionRequestError(503, { code: 'unavailable', message: 'offline' });
		s.gateway.get.mockRejectedValue(error); await expect(s.controller.boot()).rejects.toBe(error); expect(s.states).toEqual([]);
	});
	it('a non401 typed error during broadcast recovery preserves the existing view', async () => {
		const s = setup(); await s.controller.boot(); s.gateway.get.mockRejectedValue(new SessionRequestError(503, { code: 'unavailable', message: 'offline' }));
		s.broadcast({ kind: 'rotated' }); await tick(); expect(s.states).toEqual([{ kind: 'signed-in', session: view }]);
	});
	it('rejects non-object notifications even with a valid kind property', async () => {
		const s = setup(); const malformed = Object.assign(() => {}, { kind: 'rotated' }); s.broadcast(malformed); await tick();
		expect(s.store.read).not.toHaveBeenCalled();
	});
	it('stops stale boot work immediately after version negotiation', async () => {
		const s = setup(); const old = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(old.promise);
		const boot = s.controller.boot(); await tick(); await s.controller.signIn({ login: 'alex', password: 'private' });
		old.resolve({ api_version: 1, contract_revision: 2 }); await boot; expect(s.store.read).not.toHaveBeenCalled();
	});
	it('stops stale boot work immediately after the storage read', async () => {
		const s = setup(); const old = deferred<SessionRecord | undefined>(); s.store.read.mockReturnValueOnce(old.promise);
		const boot = s.controller.boot(); await tick(); await s.controller.signIn({ login: 'alex', password: 'private' });
		old.resolve(record); await boot; expect(s.gateway.get).not.toHaveBeenCalled();
	});
	it.each(['401', 'offline'] as const)('a late boot %s cannot erase or fail a newer signed-in view', async mode => {
		const s = setup(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		const boot = s.controller.boot(); await tick(); await s.controller.signIn({ login: 'alex', password: 'private' });
		old.reject(mode === '401' ? unauthorized() : new Error('offline')); await expect(boot).resolves.toBeUndefined();
		expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view });
	});
	it('stops stale broadcast recovery immediately after reading storage', async () => {
		const s = setup(); const old = deferred<SessionRecord | undefined>(); s.store.read.mockReturnValueOnce(old.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); await s.controller.signIn({ login: 'alex', password: 'private' });
		old.resolve(record); await tick(); expect(s.gateway.get).not.toHaveBeenCalled();
	});
	it('a late broadcast 401 cannot reopen reauth after a newer sign-in', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); await s.controller.signIn({ login: 'alex', password: 'private' });
		old.reject(unauthorized()); await tick(); expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view });
	});
	it('failed sign-in is never resent and leaves the existing pair and view unchanged', async () => {
		const s = setup(); await s.controller.boot(); s.gateway.signIn.mockRejectedValue(unauthorized());
		await expect(s.controller.signIn({ login: 'alex', password: 'private' })).rejects.toMatchObject({ status: 401 });
		expect(s.gateway.signIn).toHaveBeenCalledTimes(1); expect(s.store.write).not.toHaveBeenCalled();
		expect(s.stored()).toEqual(record); expect(s.states).toEqual([{ kind: 'signed-in', session: view }]); expect(s.postMessage).not.toHaveBeenCalled();
	});

	it('a view consumer cannot change the private authentication status', async () => {
		const s = setup(); await s.controller.boot(); Reflect.set(s.states[0], 'kind', 'signed-out');
		s.http.mockResolvedValue({ status: 401, body: {} }); await s.controller.read('/api/v1/capabilities');
		expect(s.states.at(-1)).toEqual({ kind: 'signed-in', session: view, reauth: 'session_ended' });
	});

	it('sign-out finding no stored pair ends locally without dispatching', async () => {
		const s = setup(); await s.controller.boot(); s.replace(undefined); await s.controller.signOut();
		expect(s.gateway.signOut).not.toHaveBeenCalled(); expect(s.states.at(-1)).toEqual({ kind: 'signed-out', reason: 'signed-out' });
	});
	it.each(['plain401', 'typed503'] as const)('refresh distinguishes %s from session expiry', async mode => {
		const s = setup(); await s.controller.boot(); const error = mode === 'plain401' ? Object.assign(new Error('offline'), { status: 401 })
			: new SessionRequestError(503, { code: 'unavailable', message: 'offline' });
		s.gateway.refresh.mockRejectedValue(error); await expect(s.controller.refresh(100)).rejects.toBe(error);
		expect(s.states).toEqual([{ kind: 'signed-in', session: view }]); expect(s.stored()).toEqual(record);
	});
	it('an untyped error with status401 on broadcast is not a session-ended signal', async () => {
		const s = setup(); await s.controller.boot(); s.gateway.get.mockRejectedValue(Object.assign(new Error('offline'), { status: 401 }));
		s.broadcast({ kind: 'rotated' }); await tick(); expect(s.states).toEqual([{ kind: 'signed-in', session: view }]);
	});
	it.each(['signIn', 'refresh', 'signOut'] as const)('forwards caller cancellation to %s without rewriting the signal', async operation => {
		const s = setup(); await s.controller.boot(); const signal = new AbortController().signal;
		if (operation === 'signIn') { const credentials = { login: 'alex', password: 'private' }; await s.controller.signIn(credentials, signal); expect(s.gateway.signIn).toHaveBeenCalledWith(credentials, signal); }
		else if (operation === 'refresh') { await s.controller.refresh(100, signal); expect(s.gateway.refresh).toHaveBeenCalledWith(proof, signal); }
		else { await s.controller.signOut(signal); expect(s.gateway.signOut).toHaveBeenCalledWith(proof, signal); }
	});

	it('an incompatible sign-in invalidates pending reads before displaying update-required', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: { result: {} } }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 });
		await expect(s.controller.signIn({ login: 'alex', password: 'private' })).rejects.toThrow('update required');
		old.resolve({ status: 200, body: { result: {} } }); await expect(read).rejects.toThrow('Session changed');
	});
	it('a session change during the 401 storage read cannot dispatch another GET', async () => {
		const s = setup(); await s.controller.boot(); const storage = deferred<SessionRecord | undefined>(); s.store.read.mockReturnValueOnce(storage.promise);
		s.http.mockResolvedValueOnce({ status: 401, body: {} }); const read = s.controller.read('/api/v1/capabilities'); await tick();
		s.gateway.signIn.mockResolvedValue({ ...view, account: { id: 'new', login: 'New' }, proof: rotated });
		await s.controller.signIn({ login: 'New', password: 'private' }); storage.resolve({ ...record, proof: rotated });
		await expect(read).rejects.toThrow('Session changed'); expect(s.http).toHaveBeenCalledTimes(1);
	});
	it('a session change during the retry rejects its late response', async () => {
		const s = setup(); await s.controller.boot(); s.replace({ ...record, proof: rotated });
		const old = deferred<{ status: number; body: unknown }>(); s.http.mockResolvedValueOnce({ status: 401, body: {} }).mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); await tick();
		s.gateway.signIn.mockResolvedValue({ ...view, account: { id: 'new', login: 'New' }, proof: rotated });
		await s.controller.signIn({ login: 'New', password: 'private' }); old.resolve({ status: 200, body: {} });
		await expect(read).rejects.toThrow('Session changed');
	});
	it('disposal removes the browser message listener', () => {
		const s = setup(); expect(s.listenerCount()).toBe(1); s.controller.dispose(); expect(s.listenerCount()).toBe(0);
	});
	it('after sign-out the private pair cannot be reused by a later read', async () => {
		const s = setup(); await s.controller.boot(); await s.controller.signOut();
		await expect(s.controller.read('/api/v1/capabilities')).rejects.toThrow('Sign in required'); expect(s.http).not.toHaveBeenCalled();
	});

	it.each(['account', 'instance'] as const)('repeat boot observes a changed %s and invalidates old reads behind Reload', async mismatch => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: unknown }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities');
		s.replace({ ...record, ...(mismatch === 'account' ? { account_id: 'blair-id', login: 'Blair' } : { instance_id: 'new-hq' }) });
		s.gateway.get.mockResolvedValue({ ...view, ...(mismatch === 'account' ? { account: { id: 'blair-id', login: 'Blair' } } : { instance_id: 'new-hq' }) });
		await s.controller.boot(); old.resolve({ status: 200, body: { result: 'Alex data' } }); await expect(read).rejects.toThrow('Session changed');
		expect(s.states.at(-1)).toMatchObject({ kind: 'signed-out', reason: 'changed-account' });
		await s.controller.boot(); expect(s.states.at(-1)).toHaveProperty('reason', 'changed-account');
	});
	it('same-account repeat boot keeps a pending read valid', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{ status: number; body: unknown }>(); s.http.mockReturnValueOnce(old.promise);
		const read = s.controller.read('/api/v1/capabilities'); await s.controller.boot(); old.resolve({ status: 200, body: {} });
		await expect(read).resolves.toMatchObject({ status: 200 });
	});
	it('a notification after version rejection cannot recover credentials', async () => {
		const s = setup(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		s.broadcast({ kind: 'rotated' }); await tick(); expect(s.states).toEqual([{ kind: 'update-required' }]);
		expect(s.store.read).not.toHaveBeenCalled(); expect(s.gateway.get).not.toHaveBeenCalled(); expect(s.gateway.version).toHaveBeenCalledTimes(1);
	});
	it.each([1, 2])('a notification shares pending boot negotiation at revision%i', async revision => {
		const s = setup(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const boot = s.controller.boot(); s.broadcast({ kind: 'rotated' }); await tick();
		expect(s.store.read).not.toHaveBeenCalled(); expect(s.gateway.get).not.toHaveBeenCalled(); expect(s.gateway.version).toHaveBeenCalledTimes(1);
		version.resolve({ api_version: 1, contract_revision: revision }); await boot; await tick();
		expect(s.states.at(-1)?.kind).toBe(revision === 1 ? 'update-required' : 'signed-in');
	});
	it('a fresh notification negotiates before touching stored credentials', async () => {
		const s = setup(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		s.broadcast({ kind: 'signed-in', account_id: record.account_id }); await tick(); expect(s.store.read).not.toHaveBeenCalled();
		version.resolve({ api_version: 1, contract_revision: 2 }); await tick(); expect(s.states.at(-1)?.kind).toBe('signed-in');
	});
	it('an older notification negotiation cannot bypass a newer rejected boot', async () => {
		const s = setup(); const old = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(old.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		old.resolve({ api_version: 1, contract_revision: 2 }); await tick(); expect(s.states).toEqual([{ kind: 'update-required' }]); expect(s.store.read).not.toHaveBeenCalled();
	});
	it('disposal while a notification negotiates prevents a later storage read', async () => {
		const s = setup(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); s.controller.dispose(); version.resolve({ api_version: 1, contract_revision: 2 }); await tick();
		expect(s.store.read).not.toHaveBeenCalled(); expect(s.states).toEqual([]);
	});
	it('sign-in negotiation invalidates a prior notification recovery immediately', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view>(); s.gateway.get.mockReturnValueOnce(old.promise);
		s.broadcast({ kind: 'rotated' }); await tick(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const login = s.controller.signIn({ login: 'alex', password: 'private' }); await tick(); old.resolve(view); await tick(); expect(s.states).toHaveLength(1);
		version.resolve({ api_version: 1, contract_revision: 2 }); await login;
	});
	it('notifications share a new sign-in negotiation rather than reusing old compatibility', async () => {
		const s = setup(); await s.controller.boot(); s.gateway.get.mockClear(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const login = s.controller.signIn({ login: 'alex', password: 'private' }); await tick(); s.broadcast({ kind: 'rotated' }); await tick(); expect(s.gateway.get).not.toHaveBeenCalled();
		const refused = expect(login).rejects.toThrow('update required'); version.resolve({ api_version: 1, contract_revision: 1 }); await refused; await tick();
		expect(s.states.at(-1)?.kind).toBe('update-required');
	});

	it.each(['signIn', 'refresh'] as const)('a dispatched %s persists its pair but cannot override newer version rejection', async operation => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view & { proof: string }>(); s.gateway[operation].mockReturnValueOnce(old.promise);
		const pending = operation === 'signIn' ? s.controller.signIn({ login: 'alex', password: 'private' }) : s.controller.refresh(100); await tick();
		s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		old.resolve({ ...view, proof: rotated }); await pending;
		expect(s.stored()?.proof).toBe(rotated); expect(s.states.at(-1)).toEqual({ kind: 'update-required' }); expect(s.postMessage).not.toHaveBeenCalled();
	});
	it.each(['signIn', 'refresh'] as const)('a %s storage completion cannot invalidate a newer boot', async operation => {
		const s = setup(); await s.controller.boot(); const write = deferred<void>();
		s.store.write.mockImplementationOnce(async value => { await write.promise; s.replace(value); });
		const pending = operation === 'signIn' ? s.controller.signIn({ login: 'alex', password: 'private' }) : s.controller.refresh(100); await tick();
		s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot(); write.resolve(); await pending;
		expect(s.stored()?.proof).toBe(rotated); expect(s.states.at(-1)).toEqual({ kind: 'update-required' }); expect(s.postMessage).not.toHaveBeenCalled();
	});
	it.each([1, 2])('an older sign-in negotiation cannot override newer revision%i boot', async revision => {
		const s = setup(); const old = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(old.promise);
		const pending = s.controller.signIn({ login: 'alex', password: 'private' }); await tick();
		s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: revision }); await s.controller.boot();
		const refused = expect(pending).rejects.toThrow('Session changed'); old.resolve({ api_version: 1, contract_revision: revision === 1 ? 2 : 1 }); await refused;
		expect(s.gateway.signIn).not.toHaveBeenCalled(); expect(s.store.write).not.toHaveBeenCalled(); expect(s.states.at(-1)?.kind).toBe(revision === 1 ? 'update-required' : 'signed-in');
	});
	it('disposal during sign-in version negotiation prevents credential dispatch', async () => {
		const s = setup(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const pending = s.controller.signIn({ login: 'alex', password: 'private' }); await tick(); s.controller.dispose();
		const refused = expect(pending).rejects.toThrow('Session changed'); version.resolve({ api_version: 1, contract_revision: 2 }); await refused;
		expect(s.gateway.signIn).not.toHaveBeenCalled(); expect(s.store.write).not.toHaveBeenCalled(); expect(s.states).toEqual([]);
	});
	it('refresh awaits a pending boot version before dispatching', async () => {
		const s = setup(); await s.controller.boot(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const boot = s.controller.boot(); const pending = s.controller.refresh(100); await tick(); expect(s.gateway.refresh).not.toHaveBeenCalled();
		version.resolve({ api_version: 1, contract_revision: 1 }); await Promise.all([boot, pending]);
		expect(s.gateway.refresh).not.toHaveBeenCalled(); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});
	it('refresh stops before dispatch when its storage read crosses a version rejection', async () => {
		const s = setup(); await s.controller.boot(); const stored = deferred<SessionRecord | undefined>(); s.store.read.mockReturnValueOnce(stored.promise);
		const pending = s.controller.refresh(100); await tick(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		stored.resolve(record); await pending; expect(s.gateway.refresh).not.toHaveBeenCalled(); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});
	it('a stale mismatched refresh cannot replace newer update-required with signed-out', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view & { proof: string }>(); s.gateway.refresh.mockReturnValueOnce(old.promise);
		const pending = s.controller.refresh(100); await tick(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		old.resolve({ ...view, account: { id: 'other', login: 'Other' }, proof: rotated }); await pending;
		expect(s.store.write).not.toHaveBeenCalled(); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});
	it('a stale refresh401 cannot publish reauth while a newer version is pending', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view & { proof: string }>(); s.gateway.refresh.mockReturnValueOnce(old.promise);
		const pending = s.controller.refresh(100); await tick(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const boot = s.controller.boot(); const refused = expect(pending).rejects.toMatchObject({ status: 401 }); old.reject(unauthorized()); await refused;
		expect(s.states).toHaveLength(1); version.resolve({ api_version: 1, contract_revision: 1 }); await boot;
	});
	it('queued refresh refuses an already rejected latest version', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view & { proof: string }>(); s.gateway.refresh.mockReturnValueOnce(old.promise);
		const first = s.controller.refresh(100); await tick(); const queued = s.controller.refresh(200);
		s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot(); old.resolve({ ...view, proof: rotated }); await Promise.all([first, queued]);
		expect(s.gateway.refresh).toHaveBeenCalledTimes(1); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});

	it.each(['missing', 'account', 'newer'] as const)('a stale refresh storage result %s cannot override newer version rejection', async change => {
		const s = setup(); await s.controller.boot(); const stored = deferred<SessionRecord | undefined>(); s.store.read.mockReturnValueOnce(stored.promise);
		const pending = s.controller.refresh(100); await tick(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		stored.resolve(change === 'missing' ? undefined : { ...record, ...(change === 'account' ? { account_id: 'other' } : { rotated_at_ms: 101 }) }); await pending;
		expect(s.states.at(-1)).toEqual({ kind: 'update-required' }); await expect(s.controller.read('/api/v1/capabilities')).rejects.toThrow('Sign in required');
	});
	it('a dispatched sign-out clears its pair without replacing a newer rejected version', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<{}>(); s.gateway.signOut.mockReturnValueOnce(old.promise);
		const pending = s.controller.signOut(); await tick(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		old.resolve({}); await pending; expect(s.stored()).toBeUndefined(); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});
	it('sign-out awaits the current pending version before dispatching', async () => {
		const s = setup(); await s.controller.boot(); const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const boot = s.controller.boot(); const pending = s.controller.signOut(); await tick(); expect(s.gateway.signOut).not.toHaveBeenCalled();
		version.resolve({ api_version: 1, contract_revision: 1 }); await Promise.all([boot, pending]); expect(s.gateway.signOut).not.toHaveBeenCalled(); expect(s.stored()).toEqual(record);
	});
	it('sign-out storage recovery cannot cross a newer version rejection', async () => {
		const s = setup(); await s.controller.boot(); const stored = deferred<SessionRecord | undefined>(); s.store.read.mockReturnValueOnce(stored.promise);
		const pending = s.controller.signOut(); await tick(); s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot();
		stored.resolve(record); await pending; expect(s.gateway.signOut).not.toHaveBeenCalled(); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});

	it.each(['refresh', 'signOut'] as const)('%s cannot read storage when a newer negotiation supersedes its version await', async operation => {
		const s = setup(); await s.controller.boot(); s.store.read.mockClear();
		const pending = operation === 'refresh' ? s.controller.refresh(100) : s.controller.signOut(); await Promise.resolve();
		const version = deferred<{ api_version: number; contract_revision: number }>(); s.gateway.version.mockReturnValueOnce(version.promise);
		const boot = s.controller.boot(); await tick(); expect(s.store.read).not.toHaveBeenCalled();
		version.resolve({ api_version: 1, contract_revision: 1 }); await Promise.all([boot, pending]);
	});
	it('queued sign-out refuses an already rejected latest version', async () => {
		const s = setup(); await s.controller.boot(); const old = deferred<typeof view & { proof: string }>(); s.gateway.refresh.mockReturnValueOnce(old.promise);
		const first = s.controller.refresh(100); await tick(); const queued = s.controller.signOut();
		s.gateway.version.mockResolvedValue({ api_version: 1, contract_revision: 1 }); await s.controller.boot(); old.resolve({ ...view, proof: rotated }); await Promise.all([first, queued]);
		expect(s.gateway.signOut).not.toHaveBeenCalled(); expect(s.states.at(-1)).toEqual({ kind: 'update-required' });
	});

});
