import { describe, expect, it, vi } from 'vitest';
import { createSessionStore } from './session-store';
import type { SessionRecord } from './session-record';

const record: SessionRecord = { instance_id: 'hq', account_id: 'alex-id', login: 'alex', proof: 'a'.repeat(64), rotated_at_ms: 100 };
type Events = Record<string, unknown>;
const fire = (target: Events, name: string) => (target[name] as () => void)();
function fixture() {
	const request: Events = { result: undefined };
	const objectStore = { get: vi.fn(() => request), put: vi.fn(() => request), delete: vi.fn(() => request) };
	const tx: Events = { objectStore: vi.fn(() => objectStore), abort: vi.fn() };
	const db = { close: vi.fn(), transaction: vi.fn(() => tx), createObjectStore: vi.fn(), onversionchange: undefined as unknown };
	const open: Events = { result: db, transaction: tx };
	const factory = { open: vi.fn(() => open) };
	const store = createSessionStore(factory as unknown as IDBFactory);
	return { request, objectStore, tx, db, open, factory, store };
}
describe('P6a IndexedDB session boundary', () => {
	it('uses the pinned database/store/key and resolves only at transaction completion', async () => {
		const s = fixture(); const pending = s.store.read(); let completed = false;
		void pending.then(() => { completed = true; });
		expect(s.factory.open).toHaveBeenCalledWith('org-session', 1);
		fire(s.open, 'onupgradeneeded'); expect(s.db.createObjectStore).toHaveBeenCalledWith('session');
		fire(s.open, 'onsuccess'); expect(s.db.transaction).toHaveBeenCalledWith('session', 'readonly');
		expect(s.objectStore.get).toHaveBeenCalledWith('current'); s.request.result = record; fire(s.request, 'onsuccess');
		for (let i = 0; i < 8; i++) await Promise.resolve(); expect(completed).toBe(false); fire(s.tx, 'oncomplete');
		expect(await pending).toEqual(record); expect(s.db.close).toHaveBeenCalledTimes(1);
	});
	it.each([undefined, null, {}, { ...record, proof: undefined }, { ...record, proof: 'invalid' }])('does not recover malformed/missing credentials %j', async value => {
		const s = fixture(); const pending = s.store.read(); fire(s.open, 'onsuccess');
		s.request.result = value; fire(s.request, 'onsuccess'); fire(s.tx, 'oncomplete');
		expect(await pending).toBeUndefined(); expect(s.objectStore.delete).not.toHaveBeenCalled();
	});
	it('validates and copies before opening a write; no mutable input or extra field is persisted', async () => {
		const s = fixture(); const input = { ...record, password: 'not-persisted' }; const pending = s.store.write(input);
		input.proof = 'b'.repeat(64); fire(s.open, 'onsuccess');
		expect(s.db.transaction).toHaveBeenCalledWith('session', 'readwrite'); expect(s.objectStore.put).toHaveBeenCalledWith(record, 'current');
		fire(s.request, 'onsuccess'); fire(s.tx, 'oncomplete'); await pending;
	});
	it('refuses a malformed write before touching the database', async () => {
		const s = fixture(); await expect(s.store.write({ ...record, proof: 'invalid' })).rejects.toThrow('Invalid gateway payload');
		expect(s.factory.open).not.toHaveBeenCalled();
	});
	it('clears only the one session key in a completed write transaction', async () => {
		const s = fixture(); const pending = s.store.clear(); fire(s.open, 'onsuccess');
		expect(s.db.transaction).toHaveBeenCalledWith('session', 'readwrite'); expect(s.objectStore.delete).toHaveBeenCalledWith('current');
		fire(s.request, 'onsuccess'); fire(s.tx, 'oncomplete'); await pending; expect(s.db.close).toHaveBeenCalledTimes(1);
	});
	it.each(['throw', 'onerror', 'onblocked'])('rejects %s during open without echoing credentials', async failure => {
		const s = fixture(); if (failure === 'throw') s.factory.open.mockImplementation(() => { throw new Error(record.proof); });
		const pending = s.store.read(); if (failure !== 'throw') { s.open.error = new Error(record.proof); fire(s.open, failure); }
		const error = await pending.catch(error => error); expect(String(error)).toBe('Error: Session storage unavailable');
		expect(String(error)).not.toContain(record.proof);
	});
	it('does not upgrade or leak a connection after a blocked open has already failed', async () => {
		const s = fixture(); const pending = s.store.read(); fire(s.open, 'onblocked'); await expect(pending).rejects.toThrow('unavailable');
		fire(s.open, 'onupgradeneeded'); expect(s.tx.abort).toHaveBeenCalled(); expect(s.db.createObjectStore).not.toHaveBeenCalled();
		fire(s.open, 'onsuccess'); expect(s.db.close).toHaveBeenCalled(); expect(s.db.transaction).not.toHaveBeenCalled();
	});
	it.each(['transaction', 'operation', 'onabort', 'onerror'])('closes and rejects a failed %s without publishing a value', async failure => {
		const s = fixture();
		if (failure === 'transaction') s.db.transaction.mockImplementation(() => { throw new Error(record.proof); });
		if (failure === 'operation') s.objectStore.get.mockImplementation(() => { throw new Error(record.proof); });
		const pending = s.store.read(); fire(s.open, 'onsuccess'); if (failure.startsWith('on')) fire(s.tx, failure);
		await expect(pending).rejects.toThrow('Session storage unavailable'); expect(s.db.close).toHaveBeenCalled();
	});
	it('releases an open connection on database version change', async () => {
		const s = fixture(); const pending = s.store.read(); fire(s.open, 'onsuccess'); (s.db.onversionchange as () => void)();
		expect(s.db.close).toHaveBeenCalled(); fire(s.tx, 'onabort'); await expect(pending).rejects.toThrow('unavailable');
	});
});
