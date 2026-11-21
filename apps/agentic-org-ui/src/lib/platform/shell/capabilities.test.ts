import { describe, expect, it } from 'vitest';
import { areaCapability } from './capabilities';

describe('production shell capabilities', () => {
	it('enables the session pages, Create organization, security settings and CLI approval, and nothing beneath them', () => {
		for (const path of [
			'/',
			'/sign-in',
			'/register',
			'/welcome',
			'/create',
			'/security',
			'/cli-approve'
		])
			expect(areaCapability(path)).toEqual({ available: true });
		expect(areaCapability('/create/x')).toMatchObject({ available: false });
		expect(areaCapability('/security/x')).toMatchObject({ available: false });
		expect(areaCapability('/cli-approve/x')).toMatchObject({ available: false });
	});
	it('keeps future product routes unavailable until an adapter is explicitly composed', () => {
		expect(areaCapability('/future-product-area')).toMatchObject({
			available: false,
			reason: 'unavailable'
		});
	});
});
