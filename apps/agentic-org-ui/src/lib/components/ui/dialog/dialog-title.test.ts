import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import Fixture from './dialog-title.test.svelte';

// These inputs contain no entities: removing only Svelte comments and our word spans
// exposes the exact serialized heading text, including whitespace. Native specs also
// verify textContent, accessible name and Selection.toString() in both engines.
function headingText(body: string) {
	const headings = [...body.matchAll(/<div\b[^>]*role="heading"[^>]*>([\s\S]*?)<\/div>/g)];
	expect(headings).toHaveLength(1);
	return headings[0][1].replace(/<!--[\s\S]*?-->/g, '').replace(/<\/?span\b[^>]*>/g, '');
}

for (const text of ['Remove Atlas from Platform', 'two two', ' two  spaces ', '\tMove\nEngineering to…\t', '', 'W'.repeat(40)]) {
	test(`title text preserves ${JSON.stringify(text)}`, () => {
		const body = render(Fixture, { props: { text } }).body;
		expect(headingText(body)).toBe(text);
		expect([...body.matchAll(/data-slot="dialog-title-word"/g)]).toHaveLength(text.match(/\S+/g)?.length ?? 0);
	});
}

test('an undefined interpolated name matches legacy Svelte text and never says undefined', () => {
	const name: string | undefined = undefined;
	const legacy = headingText(render(Fixture, { props: { legacy: true, name } }).body);
	const explicit = headingText(render(Fixture, { props: { text: `Edit ${name ?? ''}` } }).body);
	expect(legacy).toBe('Edit ');
	expect(explicit).toBe(legacy);
});

test('children remain the fallback when text is omitted', () => {
	expect(headingText(render(Fixture, { props: {} }).body)).toBe('Fallback title');
});
