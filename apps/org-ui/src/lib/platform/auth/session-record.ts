import { readSafeInteger, readSessionProof, WireDecodeError } from '../http/wire';

/** Private browser credential DTO; never publish it to UI state, logs or broadcasts. */
export interface SessionRecord {
	readonly instance_id: string;
	readonly account_id: string;
	readonly login: string;
	readonly proof: string;
	readonly rotated_at_ms: number;
}
export interface SessionStore {
	read(): Promise<SessionRecord | undefined>;
	write(record: SessionRecord): Promise<void>;
	clear(): Promise<void>;
}
/** Both reads and writes pass here; discard unknown fields before persistence. */
export function readSessionRecord(value: unknown): SessionRecord {
	try {
		if (!value || typeof value !== 'object' || Array.isArray(value) ||
			![Object.prototype, null].includes(Object.getPrototypeOf(value))) throw new WireDecodeError();
		const input = value as Record<string, unknown>;
		const text = (key: string) => {
			const item = Object.hasOwn(input, key) ? input[key] : undefined;
			if (typeof item !== 'string') throw new WireDecodeError();
			return item;
		};
		return Object.freeze({ instance_id: text('instance_id'), account_id: text('account_id'), login: text('login'),
			proof: readSessionProof(text('proof')), rotated_at_ms: readSafeInteger(Object.hasOwn(input, 'rotated_at_ms') ? input.rotated_at_ms : undefined) });
	} catch { throw new WireDecodeError(); }
}
