import { describe, expect, it } from 'vitest';
import { movedTo, redirectMoved } from './redirects';

describe('moved addresses', () => {
	it.each([
		['/agents', '/teams/members'],
		['/org-chart', '/teams/org-chart'],
		['/offices', '/settings/offices'],
		['/processes', '/settings/processes'],
		['/sessions', '/activity']
	])('%s goes to %s with its whole query string', (from, to) => {
		expect(movedTo(new URL(`http://x${from}`))).toBe(to);
		expect(movedTo(new URL(`http://x${from}?q=ada&add=1&q=two`))).toBe(`${to}?q=ada&add=1&q=two`);
	});

	it('leaves other addresses alone and redirects with SvelteKit', () => {
		expect(movedTo(new URL('http://x/teams'))).toBeUndefined();
		try {
			redirectMoved({ url: new URL('http://x/offices?office=o1') });
			expect.unreachable();
		} catch (e) {
			expect(e).toMatchObject({ status: 308, location: '/settings/offices?office=o1' });
		}
	});
});
