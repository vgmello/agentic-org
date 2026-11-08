import { describe, expect, it } from 'vitest';
import { checkTextRequest } from './check';

describe('checkTextRequest', () => {
	it('lets a request through when there is something to work from', () => {
		expect(checkTextRequest({ mode: 'improve', text: 'a rough note' })).toBeUndefined();
		expect(checkTextRequest({ mode: 'draft', text: '', instruction: 'explain the plan' })).toBeUndefined();
		expect(checkTextRequest({ mode: 'draft', text: 'my notes' })).toBeUndefined();
	});

	it('refuses to improve nothing', () => {
		expect(checkTextRequest({ mode: 'improve', text: '   ' })).toMatch(/nothing to improve/);
	});

	it('refuses to draft from nothing, even with a blank instruction', () => {
		expect(checkTextRequest({ mode: 'draft', text: '' })).toMatch(/what it should be about/);
		expect(checkTextRequest({ mode: 'draft', text: ' ', instruction: '  ' })).toMatch(/what it should be about/);
	});
});
