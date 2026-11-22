import { describe, expect, it } from 'vitest';
import { arriveWith, takeArrival } from './arrival';

describe('arriving from a save', () => {
	it('is taken once', () => {
		arriveWith({ documentId: 'doc-1', message: 'Created X for Web (team).' });
		expect(takeArrival()?.documentId).toBe('doc-1');
		expect(takeArrival()).toBeUndefined();
	});
});
