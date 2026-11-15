import { describe, expect, it } from 'vitest';
import { initialsOf } from './format';

describe('initialsOf', () => {
	it('uses the first letter of the first and last word', () => {
		expect(initialsOf('Northwind Labs')).toBe('NL');
		expect(initialsOf('Acme')).toBe('A');
		expect(initialsOf('  Inês  Duarte  ')).toBe('ID');
		expect(initialsOf('')).toBe('?');
	});
});
