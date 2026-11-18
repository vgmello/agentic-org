import { describe, expect, it } from 'vitest';
import { checkNewPassword, checkRegistration, checkSignIn, checkUsername } from './credential-rules';

// Mirrors `crates/org-host/src/accounts/rules.rs` tests; the host stays authoritative.
describe('B6 username rule mirrors the host', () => {
	it.each([
		['alex', undefined],
		['é'.repeat(64), undefined],
		['é'.repeat(64), undefined], // 128 scalars typed, 64 after NFC
		['Alex Ortiz', undefined],
		['', 'required'],
		['a'.repeat(65), 'format'],
		[' alex', 'format'],
		['alex ', 'format'],
		[' alex', 'format'],
		['al\u0007ice', 'format'],
		['al​ice', 'format'],
		['al⁤ice', 'format'],
		['al‮ice', 'format'],
		['al﻿ice', 'format'],
		['al ice', 'format'],
		['al ice', 'format']
	])('%j -> %s', (input, rule) => {
		expect(checkUsername(input)?.rule).toBe(rule);
	});
});

describe('B6 new password rule mirrors the host', () => {
	it.each([
		['a'.repeat(12), undefined],
		['\u{1F600}'.repeat(12), undefined], // 12 four-byte scalars pass: no byte minimum
		['a'.repeat(1024), undefined],
		['', 'required'],
		['a'.repeat(11), 'too_short'],
		['\u{1F600}'.repeat(11), 'too_short'],
		['é'.repeat(6), 'too_short'], // 12 scalars typed, 6 after NFC
		['a'.repeat(1025), 'too_long'],
		['\u{1F600}'.repeat(257), 'too_long'] // 1028 bytes, checked before the minimum
	])('%#', (input, rule) => {
		expect(checkNewPassword(input)?.rule).toBe(rule);
	});
});

describe('B6 form checks', () => {
	it('SIGN-3 sign-in flags only empty fields and applies no format rule', () => {
		expect(checkSignIn('', '')).toEqual([
			{ field: 'login', rule: 'required' },
			{ field: 'password', rule: 'required' }
		]);
		expect(checkSignIn(' not a valid username ', 'x')).toEqual([]);
	});
	it('SIGN-21 registration reports a mismatch only after the rule passes', () => {
		expect(checkRegistration('alex', 'long-enough-pw', 'long-enough-pX')).toEqual([{ field: 'password', rule: 'mismatch' }]);
		expect(checkRegistration('alex', 'short', 'other')).toEqual([{ field: 'password', rule: 'too_short' }]);
		expect(checkRegistration(' alex', 'long-enough-pw', 'long-enough-pw')).toEqual([{ field: 'login', rule: 'format' }]);
		expect(checkRegistration('alex', 'long-enough-pw', 'long-enough-pw')).toEqual([]);
	});
});
