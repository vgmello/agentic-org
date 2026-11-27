import { describe, expect, it } from 'vitest';
import {
	asArr,
	asBool,
	asNonEmptyStr,
	asNum,
	asRec,
	asStr,
	asStrList,
	unwrapResult
} from './decode';

describe('platform/http/decode', () => {
	it('narrows plain object records and rejects null/arrays/primitives', () => {
		expect(asRec({ a: 1 })).toEqual({ a: 1 });
		expect(asRec(null)).toBeUndefined();
		expect(asRec(undefined)).toBeUndefined();
		expect(asRec([1, 2])).toBeUndefined();
		expect(asRec('x')).toBeUndefined();
	});

	it('narrows strings, non-empty strings, finite numbers, booleans, arrays, and string lists', () => {
		expect(asStr('ok')).toBe('ok');
		expect(asStr('')).toBe('');
		expect(asStr(42)).toBeUndefined();

		expect(asNonEmptyStr('  hello  ')).toBe('hello');
		expect(asNonEmptyStr('   ')).toBeUndefined();
		expect(asNonEmptyStr(1)).toBeUndefined();

		expect(asNum(0)).toBe(0);
		expect(asNum(3.5)).toBe(3.5);
		expect(asNum(Number.NaN)).toBeUndefined();
		expect(asNum(Number.POSITIVE_INFINITY)).toBeUndefined();
		expect(asNum('3')).toBeUndefined();

		expect(asBool(false)).toBe(false);
		expect(asBool(true)).toBe(true);
		expect(asBool('true')).toBeUndefined();

		expect(asArr([1, 'a'])).toEqual([1, 'a']);
		expect(asArr('not-arr')).toEqual([]);

		expect(asStrList(['a', 1, 'b', null])).toEqual(['a', 'b']);
		expect(asStrList(null)).toEqual([]);
	});

	it('unwraps { result: {...} } envelopes and passes through bare objects', () => {
		expect(unwrapResult({ result: { id: '1' } })).toEqual({ id: '1' });
		expect(unwrapResult({ id: '2' })).toEqual({ id: '2' });
		expect(unwrapResult(null)).toBeUndefined();
		expect(unwrapResult([])).toBeUndefined();
	});
});
