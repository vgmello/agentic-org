import type { Page } from '@playwright/test';

/**
 * Resolve a CSS custom property (e.g. `--focus`, `--tone-ok-ink`) or CSS color expression
 * (`var(--surface-raised)`, `color-mix(...)`) to its computed RGB/RGBA string using independent
 * non-inheriting `background-color` probes attached to `document.documentElement`.
 *
 * Fails fast if any referenced custom property is undefined/empty, if the browser rejects the
 * expression (divergent sentinels), or if a non-transparent expression collapses to transparent.
 */
export async function resolveThemeColor(page: Page, tokenOrExpr: string): Promise<string> {
	return page.evaluate((input) => {
		const isVarName = input.startsWith('--');
		const cssExpr = isVarName ? `var(${input})` : input;
		const rootStyle = getComputedStyle(document.documentElement);
		const referencedVars = isVarName
			? [input]
			: [...input.matchAll(/var\(\s*(--[^,)\s]+)/g)].map((match) => match[1]);
		const cssWideKeywords = new Set([
			'inherit',
			'initial',
			'unset',
			'revert',
			'revert-layer',
			'currentcolor'
		]);
		const rejectKeyword = (label: string, value: string) => {
			if (cssWideKeywords.has(value.trim().toLowerCase())) {
				throw new Error(`${label} resolved to forbidden CSS keyword "${value.trim()}"`);
			}
		};
		rejectKeyword(`CSS color expression "${cssExpr}"`, cssExpr);
		for (const varName of referencedVars) {
			const raw = rootStyle.getPropertyValue(varName).trim();
			if (!raw) {
				throw new Error(`CSS custom property "${varName}" is undefined or empty on :root`);
			}
			rejectKeyword(`CSS custom property "${varName}"`, raw);
		}

		const probe = (sentinel: string) => {
			const el = document.createElement('span');
			el.style.position = 'fixed';
			el.style.visibility = 'hidden';
			el.style.backgroundColor = sentinel;
			el.style.backgroundColor = cssExpr;
			document.documentElement.append(el);
			const resolved = getComputedStyle(el).backgroundColor;
			el.remove();
			return resolved;
		};

		const a = probe('rgb(1, 2, 3)');
		const b = probe('rgb(254, 253, 252)');
		if (a !== b) {
			throw new Error(
				`CSS color expression "${cssExpr}" is invalid (sentinels diverged: "${a}" vs "${b}")`
			);
		}
		if (a === 'rgba(0, 0, 0, 0)' && cssExpr.trim() !== 'transparent') {
			throw new Error(`CSS color expression "${cssExpr}" resolved to transparent`);
		}
		return a;
	}, tokenOrExpr);
}

/** Resolve a CSS custom property or color expression to its rendered `[r, g, b, a]` canvas pixel. */
export async function resolveThemePixel(page: Page, tokenOrExpr: string): Promise<number[]> {
	const resolved = await resolveThemeColor(page, tokenOrExpr);
	return page.evaluate((color) => {
		const canvas = document.createElement('canvas');
		canvas.width = canvas.height = 1;
		const context = canvas.getContext('2d')!;
		context.fillStyle = color;
		context.fillRect(0, 0, 1, 1);
		return [...context.getImageData(0, 0, 1, 1).data];
	}, resolved);
}
