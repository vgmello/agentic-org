import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';

it('OFC-13 BUG-91: compact row thresholds use root-scaled container queries, not media queries', () => {
	const source = readFileSync(new URL('../../routes/(shell)/offices/+page.svelte', import.meta.url), 'utf8');
	const compact = source.match(/const compactRow = \{([\s\S]*?)\n\t\};/)?.[1];
	expect(compact).toBeDefined();
	// Existing sm/lg page gutters are retained. Only the compact-row rules are
	// equivalent between a computed root and a browser default of the same size.
	expect(compact).not.toMatch(/@media|\b(?:sm|md|lg|xl|2xl):|(?<!@)\b(?:min|max)-\[/);
	const thresholds = [...compact!.matchAll(/@(?:min|max)-\[([^\]]+)\]/g)];
	expect(thresholds.length).toBeGreaterThan(0);
	for (const [, threshold] of thresholds) expect(threshold).toMatch(/^\d+(?:\.\d+)?rem$/);
});
