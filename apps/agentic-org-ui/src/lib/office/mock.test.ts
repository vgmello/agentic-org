import { afterEach, describe, expect, it } from 'vitest';
import { createMockOffice } from './mock';

describe('the browser test hook', () => {
	const storage = { getItem: () => null, setItem: () => undefined };
	afterEach(() => {
		delete (globalThis as { window?: unknown }).window;
	});

	it('is not installed unless the mock asks for it (production never does)', () => {
		(globalThis as { window?: object }).window = {};
		createMockOffice({ storage });
		expect((window as unknown as Record<string, unknown>).__mockOfficeStatus).toBeUndefined();
		createMockOffice({ storage, testHook: true });
		expect(typeof (window as unknown as Record<string, unknown>).__mockOfficeStatus).toBe('function');
	});
});
