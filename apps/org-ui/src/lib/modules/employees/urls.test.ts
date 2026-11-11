import { describe, expect, it } from 'vitest';
import { agentsAddUrl, agentsSearchUrl } from './index';
import { consumeAgentsAddIntent, normalizeAgentsQuery } from './urls';

describe('Agents navigation URL contract', () => {
	it.each([
		['', ''], ['  \t\n ', ''], ['  Noor Haddad  ', 'Noor Haddad'],
		['x'.repeat(200), 'x'.repeat(200)], ['x'.repeat(201), 'x'.repeat(200)]
	])('normalizes and bounds query %j', (input, expected) => {
		expect(normalizeAgentsQuery(input)).toBe(expected);
	});
	it('exports canonical empty and Add URLs from the public module', () => {
		expect(agentsSearchUrl('   ')).toBe('/agents');
		expect(agentsAddUrl()).toBe('/agents?add=1');
	});
	it.each(['Noor Haddad', 'WEB engineer', '&add=1#/%+?', 'Inês 🙂', 'x\u0000y'])('encodes query as a single URL value: %j', (input) => {
		const url = new URL(agentsSearchUrl(input), 'http://localhost');
		expect(url.pathname).toBe('/agents');
		expect(url.searchParams.get('q')).toBe(input);
		expect([...url.searchParams.keys()]).toEqual(['q']);
		expect(url.hash).toBe('');
	});
	it('bounds builder input before URL encoding', () => {
		const url = new URL(agentsSearchUrl('  ' + 'a'.repeat(201) + '  '), 'http://localhost');
		expect(url.searchParams.get('q')).toBe('a'.repeat(200));
	});
	it('uses safe native URL encoding when the UTF-16 limit falls inside an emoji', () => {
		const url = new URL(agentsSearchUrl('a'.repeat(199) + '🙂'), 'http://localhost');
		expect(url.searchParams.get('q')).toBe('a'.repeat(199) + '\ufffd');
	});
});

describe('mounted-route Add intent consumption', () => {
	it('leaves a URL without an intent alone', () => {
		expect(consumeAgentsAddIntent(new URL('http://localhost/agents?q=Noor#roster'), { canAdd: true, alreadyOpen: false })).toBeUndefined();
	});
	it.each([
		['add=1', true], ['add=', false], ['add=0', false], ['add=true', false],
		['add=01', false], ['add=1&add=1', false], ['add=1&add=0', false], ['add=0&add=1', false]
	])('strips all values for %s and requests open=%s', (parameters, open) => {
		const original = new URL('http://localhost/agents?q=Noor&' + parameters + '#roster');
		const intent = consumeAgentsAddIntent(original, { canAdd: true, alreadyOpen: false })!;
		expect(intent.open).toBe(open);
		expect(intent.url.href).toBe('http://localhost/agents?q=Noor#roster');
		expect(original.searchParams.has('add')).toBe(true);
	});
});

it.each([
	[{ canAdd: true, alreadyOpen: false }, true],
	[{ canAdd: false, alreadyOpen: false }, false],
	[{ canAdd: true, alreadyOpen: true }, false],
	[{ canAdd: false, alreadyOpen: true }, false]
])('applies mounted-route authority %j', (authority, open) => {
	const intent = consumeAgentsAddIntent(new URL('http://localhost/agents?add=1'), authority)!;
	expect(intent.open).toBe(open);
	expect(intent.url.searchParams.has('add')).toBe(false);
});
