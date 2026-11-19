import { describe, expect, it } from 'vitest';
import { codesMatch, groupCode, readCheckCode } from './check-code';

describe('readCheckCode', () => {
	it('ignores case, spaces and dashes', () => {
		expect(readCheckCode('k7qp-4mzt').code).toBe('K7QP4MZT');
		expect(readCheckCode(' k7qp 4mzt ').complete).toBe(true);
	});
	it('reads O as 0, and I and L as 1', () => {
		expect(readCheckCode('OIL').code).toBe('011');
	});
	it('rejects U and characters that cannot be in a code, as typed', () => {
		const read = readCheckCode('K7QU');
		expect(read.code).toBe('K7Q');
		expect(read.rejected).toBe(true);
		expect(readCheckCode('K7Q@').rejected).toBe(true);
	});
	it('is complete only at 8 characters, and takes no more', () => {
		expect(readCheckCode('K7QP4MZ').complete).toBe(false);
		expect(readCheckCode('K7QP4MZTX').code).toBe('K7QP4MZT');
	});
});

describe('codesMatch', () => {
	it('matches look-alikes', () => {
		expect(codesMatch('k7qp 4mzt', 'K7QP4MZT')).toBe(true);
		expect(codesMatch('0O0O0O0O', 'OOOOOOOO')).toBe(true);
		expect(codesMatch('K7QP4MZ1', 'K7QP4MZT')).toBe(false);
	});
});

describe('groupCode', () => {
	it('groups four and four', () => {
		expect(groupCode('K7QP4MZT')).toBe('K7QP 4MZT');
	});
});
