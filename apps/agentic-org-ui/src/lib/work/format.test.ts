import { describe, expect, it } from 'vitest';
import { environmentLine, shortDate } from './format';

describe('format', () => {
	it('writes a short date and an environment that is not a user agent', () => {
		expect(shortDate('2026-11-08T10:00:00.000Z')).toBe('8 Nov');
		expect(environmentLine()).toMatch(/^This Mac \(HQ\) · Agentic Org \d+\.\d+\.\d+$/);
		expect(environmentLine()).not.toContain('Mozilla');
	});
});
