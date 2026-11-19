import { describe, expect, it } from 'vitest';
import { agoText, expiresText, madeAtText } from './time';

const now = Date.parse('2026-11-18T12:00:00Z');
describe('agoText', () => {
	it('rounds down', () => {
		expect(agoText('2026-11-18T11:58:30Z', now)).toBe('1 min ago');
		expect(agoText('2026-11-18T09:50:00Z', now)).toBe('2 h ago');
		expect(agoText('2026-11-18T11:59:50Z', now)).toBe('just now');
	});
});
describe('expiresText', () => {
	it('counts down and says expired', () => {
		expect(expiresText('2026-11-19T09:30:00Z', now)).toBe('expires in 21 h');
		expect(expiresText('2026-11-18T12:12:00Z', now)).toBe('expires in 12 min');
		expect(expiresText('2026-11-18T11:00:00Z', now)).toBe('expired');
	});
});

describe('madeAtText', () => {
	it('names the day when it is not today', () => {
		const local = (d: number, h: number, m: number) => new Date(2026, 10, d, h, m).getTime();
		expect(madeAtText(new Date(local(18, 14, 2)).toISOString(), local(18, 15, 0))).toBe(
			'today at 14:02'
		);
		expect(madeAtText(new Date(local(17, 23, 0)).toISOString(), local(18, 8, 0))).toBe(
			'yesterday at 23:00'
		);
		expect(madeAtText(new Date(local(16, 9, 15)).toISOString(), local(18, 8, 0))).toMatch(
			/^on .+ at 09:15$/
		);
	});
});
