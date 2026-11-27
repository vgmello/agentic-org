import { describe, expect, it } from 'vitest';
import { safeInternalPath, splitSafeHref } from './safe-url';

describe('safeInternalPath', () => {
	it('accepts clean app-relative paths with optional query and hash', () => {
		expect(safeInternalPath('/work/T-12?tab=comments')).toBe('/work/T-12?tab=comments');
		expect(safeInternalPath('/work/T-12#top', { allowHash: true })).toBe('/work/T-12#top');
		expect(safeInternalPath('/work/T-12#secret')).toBe('/work/T-12');
	});

	it('rejects external URLs, protocol-relative paths, backslashes, control chars, dot traversal, and route templates', () => {
		for (const bad of [
			'',
			'https://evil.example/x',
			'javascript:alert(1)',
			'//evil.example/x',
			'/\\evil.example',
			'/work/\n/evil',
			'/work/../admin',
			'/work/%2e%2e/admin',
			'/projects/[id]/work',
			'/(shell)/home'
		]) {
			expect(safeInternalPath(bad), bad).toBeUndefined();
		}
		expect(safeInternalPath('/api/v1/sessions', { disallowApi: true })).toBeUndefined();
	});

	it('splits pathname and query/hash suffix cleanly', () => {
		expect(splitSafeHref('/inbox?tab=approvals#item-1')).toEqual({
			pathname: '/inbox',
			suffix: '?tab=approvals#item-1'
		});
		expect(splitSafeHref('/work')).toEqual({ pathname: '/work', suffix: '' });
	});
});
