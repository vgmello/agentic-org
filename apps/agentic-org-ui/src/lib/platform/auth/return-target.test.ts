import { describe, expect, it } from 'vitest';
import { safeReturnTarget, signInHref } from './return-target';

const origin = 'http://127.0.0.1:7420';
describe('B6 SIGN-8 return targets', () => {
	it.each([
		['/agents', '/agents'],
		['/projects/7?tab=work', '/projects/7?tab=work'],
		['/chat#secret', '/chat'],
		['//evil.example/x', undefined],
		['/\\evil.example', undefined],
		['https://evil.example/', undefined],
		['http://127.0.0.1:7420/agents', undefined],
		['javascript:alert(1)', undefined],
		['/api/v1/session', undefined],
		['/sign-in', undefined],
		['/register', undefined],
		['/a b', undefined],
		['/..//evil.example/x', undefined],
		['/.//evil.example', undefined],
		['/%2e%2e//evil', undefined],
		['/%2E.//evil', undefined],
		['/a/../..//evil', undefined],
		['/a/./b', undefined],
		['/a/..', undefined],
		['/a//b', '/a//b'],
		['/files/..hidden', '/files/..hidden'],
		['/search?q=/../x', '/search?q=/../x'],
		['', undefined],
		[null, undefined]
	])('%j -> %j', (raw, expected) => {
		expect(safeReturnTarget(raw, origin)).toBe(expected);
	});
	it('builds the sign-in URL with a valid target and note only', () => {
		expect(signInHref('/agents', origin, 'ended')).toBe('/sign-in?next=%2Fagents&note=ended');
		expect(signInHref('//evil', origin)).toBe('/sign-in');
		expect(signInHref('/', origin, 'signed-out')).toBe('/sign-in?note=signed-out');
	});
});
