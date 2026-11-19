import { readSessionRecord, type SessionRecord, type SessionStore } from './session-record';

/** One cookie/proof pair per browser origin. Transaction completion precedes publication. */
export function createSessionStore(factory: IDBFactory): SessionStore {
	function transaction<T>(mode: IDBTransactionMode, operation: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
		return new Promise((resolve, reject) => {
			let settled = false;
			const fail = () => { settled = true; reject(new Error('Session storage unavailable')); };
			let open: IDBOpenDBRequest;
			try { open = factory.open('org-session', 1); } catch { fail(); return; }
			open.onblocked = fail;
			open.onerror = fail;
			open.onupgradeneeded = () => {
				if (settled) { open.transaction?.abort(); return; }
				open.result.createObjectStore('session');
			};
			open.onsuccess = () => {
				const db = open.result;
				if (settled) { db.close(); return; }
				db.onversionchange = () => db.close();
				try {
					const tx = db.transaction('session', mode);
					let result: T;
					tx.oncomplete = () => { db.close(); settled = true; resolve(result); };
					tx.onabort = () => { db.close(); fail(); };
					tx.onerror = () => { db.close(); fail(); };
					const request = operation(tx.objectStore('session'));
					request.onsuccess = () => { result = request.result; };
				} catch { db.close(); fail(); }
			};
		});
	}
	return Object.freeze({
		async read() {
			const value = await transaction('readonly', store => store.get('current'));
			if (value === undefined) return undefined;
			// Missing/malformed proof ends local recovery; there is no cookie-only fallback.
			try { return readSessionRecord(value); } catch { return undefined; }
		},
		async write(record: SessionRecord) {
			const checked = readSessionRecord(record);
			await transaction('readwrite', store => store.put(checked, 'current'));
		},
		async clear() { await transaction('readwrite', store => store.delete('current')); }
	});
}
