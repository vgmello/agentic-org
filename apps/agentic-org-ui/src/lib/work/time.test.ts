import { describe, expect, it } from 'vitest';
import { span } from './time';

describe('TSK-11 the two largest units', () => {
	it('writes days and hours, hours and minutes, minutes, under a minute, and nothing as "0 min"', () => {
		expect(span(28 * 3_600_000)).toBe('1 d 4 h');
		expect(span(3 * 3_600_000)).toBe('3 h');
		expect(span(5 * 60_000)).toBe('5 min');
		expect(span(30_000)).toBe('<1 min');
		expect(span(0)).toBe('0 min');
	});
});
