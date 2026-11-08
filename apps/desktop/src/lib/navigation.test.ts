import { describe, expect, it } from 'vitest';
import { isAppPath } from './navigation';

describe('isAppPath', () => {
	it('accepts the screens the menu bar opens and nothing else', () => {
		for (const ok of ['/', '/welcome', '/create', '/agents', '/offices']) expect(isAppPath(ok)).toBe(true);
		for (const bad of ['/etc/passwd', 'https://example.com', '//evil.com', '/agents/../x', '', null, undefined, 3])
			expect(isAppPath(bad)).toBe(false);
	});
});
