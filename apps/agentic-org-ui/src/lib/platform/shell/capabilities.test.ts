import { describe, expect, it } from 'vitest';
import { areaCapability } from './capabilities';

describe('production shell capabilities', () => {
	it('keeps future product routes unavailable until an adapter is explicitly composed', () => {
		expect(areaCapability('/future-product-area')).toMatchObject({ available: false, reason: 'unavailable' });
	});
});
