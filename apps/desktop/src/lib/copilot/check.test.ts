import { describe, expect, it } from 'vitest';
import { checkTextRequest, fitTo } from './check';

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

describe('fitTo', () => {
	it('leaves a text that fits, and a field with no limit', () => {
		expect(fitTo('abc', 3)).toEqual({ text: 'abc', clipped: false });
		expect(fitTo('abc', undefined)).toEqual({ text: 'abc', clipped: false });
	});

	it('cuts a longer text to the limit and says so', () => {
		expect(fitTo('abcdef', 4)).toEqual({ text: 'abcd', clipped: true });
	});

	it('never leaves half of a character at the end', () => {
		expect(fitTo('ab😀', 3)).toEqual({ text: 'ab', clipped: true });
		expect(fitTo('ab😀', 4)).toEqual({ text: 'ab😀', clipped: false });
	});
});
