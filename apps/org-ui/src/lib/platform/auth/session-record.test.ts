import { describe, expect, it } from 'vitest';
import { readSessionRecord } from './session-record';
import { WireDecodeError } from '../http/wire';

const valid = { instance_id: 'hq', account_id: 'alex-id', login: 'alex', proof: 'a'.repeat(64), rotated_at_ms: 100 };
describe('P6a browser session record', () => {
	it('copies only the five record fields and freezes the validated credential DTO', () => {
		const input = { ...valid, password: 'never-persist', extra: {} };
		const result = readSessionRecord(input);
		expect(result).toEqual(valid);
		expect(result).not.toBe(input);
		expect(Object.isFrozen(result)).toBe(true);
		input.proof = 'b'.repeat(64);
		expect(result.proof).toBe(valid.proof);
	});
	it('accepts a null-prototype record and safe integer bounds', () => {
		for (const rotated_at_ms of [0, Number.MAX_SAFE_INTEGER]) {
			const value = Object.assign(Object.create(null), valid, { rotated_at_ms });
			expect(readSessionRecord(value).rotated_at_ms).toBe(rotated_at_ms);
		}
	});
	it.each([null, false, 1, '', [], Object.create(valid)])('refuses non-record or inherited records: %j', value => {
		expect(() => readSessionRecord(value)).toThrow(WireDecodeError);
	});
	it.each(['instance_id', 'account_id', 'login', 'proof', 'rotated_at_ms'])('requires own %s', key => {
		const value: Record<string, unknown> = { ...valid };
		delete value[key];
		expect(() => readSessionRecord(value)).toThrow(WireDecodeError);
	});
	it.each(['instance_id', 'account_id', 'login', 'proof'])('refuses non-text %s', key => {
		expect(() => readSessionRecord({ ...valid, [key]: 1 })).toThrow(WireDecodeError);
	});
	it.each(['a'.repeat(63), 'a'.repeat(65), 'A'.repeat(64), 'g'.repeat(64), ' '.repeat(64), undefined])('requires canonical proof %j', proof => {
		expect(() => readSessionRecord({ ...valid, proof })).toThrow(WireDecodeError);
	});
	it.each([-1, 0.5, Number.MAX_SAFE_INTEGER + 1, Infinity, NaN, '100', undefined])('requires a safe nonnegative rotation time %j', rotated_at_ms => {
		expect(() => readSessionRecord({ ...valid, rotated_at_ms })).toThrow(WireDecodeError);
	});
	it('never echoes a throwing getter, proxy or secret in a decode error', () => {
		for (const value of [
			{ ...valid, get login() { throw new Error(valid.proof); } },
			new Proxy({}, { getPrototypeOf() { throw new Error(valid.proof); } })
		]) {
			let error: unknown;
			try { readSessionRecord(value); } catch (caught) { error = caught; }
			expect(error).toBeInstanceOf(WireDecodeError);
			expect(String(error)).not.toContain(valid.proof);
		}
	});
	it('refuses arrays, functions and custom prototypes even with all valid own fields', () => {
		for (const value of [Object.assign([], valid), Object.assign(Object.setPrototypeOf([], Object.prototype), valid), Object.assign(Object.setPrototypeOf(() => {}, null), valid), Object.assign(Object.create({ extra: true }), valid)])
			expect(() => readSessionRecord(value)).toThrow(WireDecodeError);
	});
	it.each(['instance_id', 'account_id', 'login', 'proof', 'rotated_at_ms'])('does not inherit polluted Object.prototype %s', key => {
		const value: Record<string, unknown> = { ...valid }; const inherited = value[key]; delete value[key];
		const original = Object.getOwnPropertyDescriptor(Object.prototype, key);
		try {
			Object.defineProperty(Object.prototype, key, { value: inherited, writable: true, configurable: true });
			expect(() => readSessionRecord(value)).toThrow(WireDecodeError);
		} finally {
			if (original) Object.defineProperty(Object.prototype, key, original); else Reflect.deleteProperty(Object.prototype, key);
		}
	});

	it('reads each credential getter exactly once before persisting its checked value', () => {
		const calls: Record<string, number> = {};
		const input = Object.fromEntries(Object.entries(valid).map(([key, value]) => [key, value]));
		for (const [key, value] of Object.entries(valid)) Object.defineProperty(input, key, {
			get() { calls[key] = (calls[key] ?? 0) + 1; return calls[key] === 1 ? value : { invalid: true }; }
		});
		expect(readSessionRecord(input)).toEqual(valid);
		expect(Object.values(calls)).toEqual(Array(5).fill(1));
	});

});
