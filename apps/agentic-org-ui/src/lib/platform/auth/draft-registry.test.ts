import { describe, expect, it } from 'vitest';
import { createDraftRegistry } from './draft-registry';

describe('B6 draft registry (SIGN-15, SIGN-19)', () => {
	it('is true only while a registered page reports unsaved text', () => {
		const registry = createDraftRegistry();
		expect(registry.hasEditable()).toBe(false);
		expect(registry.hasUnsavedText()).toBe(false);
		let text = '';
		const unregister = registry.register(() => text.length > 0);
		expect(registry.hasEditable()).toBe(true);
		expect(registry.hasUnsavedText()).toBe(false);
		text = 'draft';
		expect(registry.hasUnsavedText()).toBe(true);
		unregister();
		expect(registry.hasUnsavedText()).toBe(false);
		expect(registry.hasEditable()).toBe(false);
	});
	it('treats a failing probe as unsaved, so sign-out still asks first', () => {
		const registry = createDraftRegistry();
		registry.register(() => {
			throw new Error('gone');
		});
		expect(registry.hasUnsavedText()).toBe(true);
	});
});
