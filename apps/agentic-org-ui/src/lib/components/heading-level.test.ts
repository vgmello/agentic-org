import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import EmptyState from './EmptyState.svelte';
import ErrorState from './ErrorState.svelte';

/** Server-rendered markup without Svelte's hydration comments. */
const html = (body: string) => body.replace(/<!--[\s\S]*?-->/g, '');

test.each([2, 3, 4] as const)('empty and error states take heading level %i', (level) => {
	expect(html(render(EmptyState, { props: { title: 'Nothing here', level } }).body)).toMatch(
		new RegExp(`<h${level}[^>]*>Nothing here</h${level}>`)
	);
	expect(html(render(ErrorState, { props: { title: "Couldn't load", level } }).body)).toMatch(
		new RegExp(`<h${level}[^>]*>Couldn't load</h${level}>`)
	);
});

test('the error heading stays an h2 by default', () => {
	expect(html(render(ErrorState, { props: { title: "Couldn't load" } }).body)).toMatch(
		/<h2[^>]*>Couldn't load<\/h2>/
	);
});
