import { describe, expect, it, vi } from 'vitest';
import { takeLinkSecret } from './link-fragment';

describe('B6 registration link fragment', () => {
	it('returns the secret and clears the hash before anything else', () => {
		const replaceState = vi.fn();
		expect(takeLinkSecret({ hash: '#k7Q2abc', pathname: '/register', search: '' }, { replaceState, state: null })).toBe('k7Q2abc');
		expect(replaceState).toHaveBeenCalledWith(null, '', '/register');
	});
	it('clears even an unusable fragment, and returns nothing for none', () => {
		const replaceState = vi.fn();
		expect(takeLinkSecret({ hash: '#a b', pathname: '/register', search: '?x=1' }, { replaceState, state: null })).toBeUndefined();
		expect(replaceState).toHaveBeenCalledWith(null, '', '/register?x=1');
		const untouched = vi.fn();
		expect(takeLinkSecret({ hash: '', pathname: '/register', search: '' }, { replaceState: untouched, state: null })).toBeUndefined();
		expect(untouched).not.toHaveBeenCalled();
	});
});
