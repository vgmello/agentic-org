import { describe, expect, it } from 'vitest';
import { handleProblem, nameProblem, suggestHandle } from './person';

describe('adding a person from a team', () => {
	it('suggests a lowercase handle from the name', () => {
		expect(suggestHandle('Sam Ortiz')).toBe('sam-ortiz');
		expect(suggestHandle('  Zoë  O’Brien ')).toBe('zoe-o-brien');
		expect(suggestHandle('!!!')).toBe('');
	});
	it('checks the handle form and keeps reserved addresses', () => {
		expect(handleProblem('sam-ortiz')).toBeUndefined();
		expect(handleProblem('s.o_2')).toBeUndefined();
		expect(handleProblem('')).toBe('Choose a handle.');
		expect(handleProblem('@sam')).toMatch(/Leave out the @/);
		expect(handleProblem('Sam')).toBe('Use lowercase letters.');
		expect(handleProblem('-sam')).toMatch(/starting with a letter or digit/);
		expect(handleProblem('sam ortiz')).toMatch(/Use letters/);
		expect(handleProblem('a'.repeat(33))).toBe('Keep it to 32 characters.');
		expect(handleProblem('owner')).toBe('@owner is reserved. Choose another.');
	});
	it('requires a name without control characters', () => {
		expect(nameProblem(' ')).toBe('Enter their name.');
		expect(nameProblem('Sam\u0007')).toBe('Remove the control characters.');
		expect(nameProblem('Sam Ortiz')).toBeUndefined();
	});
});
