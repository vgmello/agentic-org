import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join, relative } from 'node:path';
import { expect, it } from 'vitest';

// Rowan E3: bare copyright/registered/trademark are typography. Variation
// selectors, default emoji presentation, flags and keycaps are UI emoji.
const emoji = /\p{Regional_Indicator}{2}|\p{Emoji_Presentation}|\p{Extended_Pictographic}\uFE0F|[#*0-9]\uFE0F?\u20E3/gu;
const exclusions = ['lib/modules/copilot/check.test.ts', 'lib/modules/messaging/engine.test.ts', 'lib/modules/employees/urls.test.ts'];
const root = fileURLToPath(new URL('../../..', import.meta.url));
const glyphs = (text: string) => [...text.replace(/<!--[\s\S]*?-->/g, '').matchAll(/>\s*([×↳›✓★✕])\s*</gu)].map((match) => match[1]);
const matches = (text: string) => [...text.matchAll(emoji)].map((match) => match[0]);
function inspectSource(root: string) {
	// An exact allowlist of inventory Unicode fixtures; missing files fail loudly.
	for (const file of exclusions) readFileSync(join(root, file), 'utf8');
	const problems: string[] = [];
	function scan(directory: string) {
		for (const entry of readdirSync(directory, { withFileTypes: true })) {
			const path = join(directory, entry.name), name = relative(root, path);
			if (entry.isDirectory()) scan(path);
			else if (!exclusions.includes(name)) {
				const text = readFileSync(path, 'utf8');
				const found = [...matches(text), ...(name.endsWith('.svelte') ? glyphs(text) : [])];
				if (found.length) problems.push(`${name}: ${found.join(' ')}`);
			}
		}
	}
	scan(root);
	return problems;
}
function fixture(check: (root: string, put: (name: string, text: string) => void) => void) {
	const root = mkdtempSync(join(tmpdir(), 'org-ui-no-emoji-'));
	const put = (name: string, text: string) => { const path = join(root, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, text); };
	try {
		for (const name of exclusions) put(name, '\u{1f4ce}');
		check(root, put);
	} finally { rmSync(root, { recursive: true, force: true }); }
}

it('E1–E3: shipped UI source, sample and mock data contain no emoji or glyph-only pseudo-icons', () => {
	expect(inspectSource(root)).toEqual([]);
});
it('E3: permits ordinary typography, including bare copyright, registered and trademark', () => {
	expect(matches('#general 14 * 2 @owner A → B Engineering › Web · ⌘ \u00a9 \u00ae \u2122')).toEqual([]);
});
it.each(['\u2705', '\u00a9\ufe0f', '\u{1f1ec}\u{1f1e7}', '1\ufe0f\u20e3', '#\u20e3'])('E3: rejects emoji presentation %s', (text) => {
	expect(matches(text)).toEqual([text]);
});
it.each([
	['components/Example.svelte', '<button>\u{1f4ce}</button>'],
	['lib/org/sample.ts', "export const name = '\u{1f600}';"],
	['lib/mock-data.json', '{"label":"\u{1f469}\u200d\u{1f4bb}"}']
])('E3 negative control: reports an emoji introduced in %s', (name, text) => fixture((root, put) => {
	put(name, text);
	expect(inspectSource(root)).toEqual([`${name}: ${matches(text).join(' ')}`]);
}));
it('E3: excludes only the three named Unicode fixtures, including no blanket test exclusion', () => fixture((root, put) => {
	expect(inspectSource(root)).toEqual([]);
	put('lib/new.test.ts', '\u{1f4ce}');
	expect(inspectSource(root)).toEqual(['lib/new.test.ts: \u{1f4ce}']);
}));
it('E3: a removed allowlisted fixture fails rather than silently rotting', () => fixture((root) => {
	rmSync(join(root, exclusions[0]));
	expect(() => inspectSource(root)).toThrow(/ENOENT/);
}));

it.each(['×', '↳', '›', '✓', '★', '✕'])('E3 pseudo-icons: reports a glyph-only Svelte element %s', (glyph) => fixture((root, put) => {
	put('components/Example.svelte', `<button> \n${glyph}\t </button>`);
	expect(inspectSource(root)).toEqual([`components/Example.svelte: ${glyph}`]);
}));
it('E3 pseudo-icons: permits text-label typography, comments and non-Svelte text', () => fixture((root, put) => {
	put('components/Example.svelte', '<span>Engineering › Web</span><span>Engineering ›</span><span>› Web</span><span>@owner</span><span>#general</span><!-- <button>×</button> -->');
	put('lib/example.ts', "const label = '<button>×</button>';");
	expect(inspectSource(root)).toEqual([]);
}));

it('E3 pseudo-icons: strips each comment without hiding a glyph between comments', () => fixture((root, put) => {
	put('components/Example.svelte', '<!-- before --><button>×</button><!-- after -->');
	expect(inspectSource(root)).toEqual(['components/Example.svelte: ×']);
}));
