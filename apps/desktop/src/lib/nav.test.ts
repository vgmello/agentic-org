import { describe, expect, it } from 'vitest';
import { NAV, navFor, titleFor } from './nav';

describe('titleFor', () => {
	it('is the section name for a sidebar page', () => {
		expect(titleFor('/org-chart')).toBe('Org chart');
		expect(titleFor('/offices/12')).toBe('Offices');
	});

	it('names the Report a bug page, which is not in the sidebar', () => {
		expect(titleFor('/report-bug')).toBe('Report a bug');
		expect(navFor('/report-bug')).toBeUndefined();
		expect(NAV.some((item) => item.href === '/report-bug')).toBe(false);
	});

	it('is undefined for a page it does not know', () => {
		expect(titleFor('/nope')).toBeUndefined();
	});
});
