import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { allowlist, findRaw, rewrite, tokenFor } from './look-tokens';

// Every colour comes from a look token (src/app.css; README-screens.md "Look tokens"). This guard
// fails on a raw Tailwind palette class (text-gray-400, bg-white/5, outline-indigo-400 …) or a
// raw colour literal anywhere in src/ outside the allowlist in look-tokens.ts, and names the token
// to use. `bun scripts/look-tokens.ts` rewrites them.
const app = fileURLToPath(new URL('../../../..', import.meta.url));

function sources(dir: string): string[] {
	const out: string[] = [];
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) out.push(...sources(path));
		else if (/\.(svelte|ts|js|css|html)$/.test(entry.name)) out.push(path);
	}
	return out;
}

/** Colour literals that bypass the tokens: CSS colour functions, hex in CSS, palette variables, SVG paint names. */
const literal =
	/\b(?:rgba?|hsla?|oklch|oklab|lab|lch|hwb)\(|var\(--color-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|white|black)\b|\b(?:fill|stroke|color|stop-color)="(?:white|black|#[0-9a-fA-F]{3,8})"/g;
const cssHex = /#[0-9a-fA-F]{3,8}\b/g;

function lineOf(text: string, index: number) {
	return text.slice(0, index).split('\n').length;
}

describe('look tokens guard', () => {
	it('src/ uses look tokens, not raw palette classes or colour literals', () => {
		const problems: string[] = [];
		for (const path of sources(join(app, 'src'))) {
			const name = relative(app, path);
			if (allowlist[name]) continue;
			const text = readFileSync(path, 'utf8');
			for (const raw of findRaw(text)) {
				problems.push(
					`${name}:${raw.line}  ${raw.class}  → use ${raw.use ?? 'a look token (add one to src/app.css and look-tokens.ts)'}`
				);
			}
			for (const m of text.matchAll(literal))
				problems.push(
					`${name}:${lineOf(text, m.index ?? 0)}  ${m[0]}  → use a look token from src/app.css`
				);
			if (
				name.endsWith('.css') ||
				(name.endsWith('.ts') &&
					!name.endsWith('.test.ts') &&
					!name.startsWith('src/lib/platform/tooling/'))
			) {
				const hexRe = name.endsWith('.css') ? cssHex : /['"`]#[0-9a-fA-F]{3,8}['"`]/g;
				for (const m of text.matchAll(hexRe))
					problems.push(
						`${name}:${lineOf(text, m.index ?? 0)}  ${m[0]}  → use a look token from src/app.css`
					);
			}
		}
		expect(
			problems,
			`Raw colours found. Use the look tokens (README-screens.md "Look tokens"), or run \`bun scripts/look-tokens.ts\`:\n${problems.join('\n')}`
		).toEqual([]);
	});

	it('allowlist entries exist and each has a reason', () => {
		for (const [file, reason] of Object.entries(allowlist)) {
			expect(() => readFileSync(join(app, file), 'utf8'), file).not.toThrow();
			expect(reason.length, file).toBeGreaterThan(10);
		}
	});

	it('every token the table can produce is defined as a colour utility in app.css', () => {
		const css = readFileSync(join(app, 'src/app.css'), 'utf8');
		const defined = new Set([...css.matchAll(/--color-([a-z-]+):/g)].map((m) => m[1]));
		const colours = ['white', 'black'];
		for (const family of ['gray', 'indigo', 'red', 'amber', 'green', 'emerald', 'sky']) {
			for (const shade of ['100', '200', '300', '400', '500', '600', '700', '800', '900', '950'])
				colours.push(`${family}-${shade}`);
		}
		const missing = new Set<string>();
		for (const prefix of ['text', 'bg', 'border', 'outline', 'ring', 'fill', 'accent', 'divide']) {
			for (const colour of colours) {
				for (const opacity of [undefined, '5', '10', '20', '[0.03]']) {
					for (const variants of ['', 'backdrop:']) {
						for (const context of ['', 'bg-indigo-500']) {
							const token = tokenFor({ prefix, colour, opacity, variants, context });
							if (!token) continue;
							const name = token.slice(prefix.length + 1).split('/')[0];
							if (!defined.has(name)) missing.add(`${prefix}-${colour} → ${token}`);
						}
					}
				}
			}
		}
		expect([...missing]).toEqual([]);
	});
});

describe('look tokens codemod', () => {
	it('maps by role and keeps variants and opacity', () => {
		const input = `<p class="text-gray-400 hover:text-white border-white/5 bg-red-500/10 focus-visible:outline-indigo-400 dark:bg-white/5">`;
		expect(rewrite(input, 'x.svelte').text).toBe(
			`<p class="text-fg-muted hover:text-fg border-line-subtle bg-danger-emphasis/10 focus-visible:outline-focus dark:bg-surface-raised">`
		);
	});

	it('rewrites class: directives, ternaries, cn() strings and helper constants', () => {
		const input = [
			`<span class:text-amber-200={owner} class={cn('ring-white/10', open ? 'bg-indigo-500 text-white' : 'text-white')}></span>`,
			`export const tone = { bad: 'bg-red-500/10 text-red-300 ring-red-500/20' } as const;`,
			"const c = `divide-y divide-white/5 ${x ? 'bg-white/[0.03]' : ''}`;"
		].join('\n');
		expect(rewrite(input, 'x.svelte').text).toBe(
			[
				`<span class:text-warning-text-strong={owner} class={cn('ring-line', open ? 'bg-primary text-on-emphasis' : 'text-fg')}></span>`,
				`export const tone = { bad: 'bg-danger-emphasis/10 text-danger-text ring-danger-emphasis/20' } as const;`,
				"const c = `divide-y divide-line-subtle ${x ? 'bg-tint/[0.03]' : ''}`;"
			].join('\n')
		);
	});

	it('tells one palette colour apart by utility prefix and variant', () => {
		const { text } = rewrite(
			`"text-gray-500 outline-gray-500 bg-gray-500 bg-gray-950 backdrop:bg-gray-950/80 text-indigo-400 outline-indigo-400 border-indigo-400"`,
			'x.svelte'
		);
		expect(text).toBe(
			`"text-fg-subtle outline-field-border bg-quiet-emphasis bg-tooltip backdrop:bg-scrim/80 text-link outline-focus border-brand"`
		);
	});

	it('is idempotent and reports what it cannot map', () => {
		const once = rewrite(`"text-gray-400 bg-pink-500 text-gray-600"`, 'x.svelte');
		expect(once.text).toBe(`"text-fg-muted bg-pink-500 text-gray-600"`);
		expect(once.unmapped.map((u) => u.class)).toEqual(['bg-pink-500', 'text-gray-600']);
		const twice = rewrite(once.text, 'x.svelte');
		expect(twice.text).toBe(once.text);
		expect(twice.changes).toEqual([]);
	});

	it('leaves token names and look-alike words alone', () => {
		const input = `<p class="text-fg-muted bg-surface-raised line-subtle border-line text-white-ish bg-gray-400px">`;
		expect(rewrite(input, 'x.svelte').changes).toEqual([]);
	});
});
