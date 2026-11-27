export interface SuppressionCounts {
	eslint: number;
	svelte: number;
	effect: number;
}

export interface SuppressionScanResult {
	counts: SuppressionCounts;
	errors: string[];
}

const LINE_SCOPED_ESLINT_RE = new RegExp(
	'(?:\\/\\/|\\/\\*|<!--)\\s*eslint-disable-(?:next-line|line)\\b([^\\n]*)',
	'g'
);
const BLOCK_ESLINT_DISABLE_RE = new RegExp(
	'(?:\\/\\/|\\/\\*|<!--)\\s*eslint-disable(?!-(?:next-line|line)\\b)\\b',
	'g'
);
const INLINE_ESLINT_CONFIG_RE = new RegExp(
	'(?:\\/\\*|<!--)\\s*(?:eslint|eslint-env|globals?|exported)\\b',
	'g'
);
const SVELTE_IGNORE_LINE_RE = new RegExp(
	'(?:\\/\\/|\\/\\*|<!--)\\s*svelte-ignore\\b([^\\n]*)',
	'g'
);
const EFFECT_CALL_RE = new RegExp(['\\$', 'effect(?:\\.pre)?\\s*\\('].join(''), 'g');

function hasEslintReason(tail: string): boolean {
	const cleaned = tail.replace(/(?:\*\/|-->)\s*$/, '').trim();
	const idx = cleaned.indexOf('--');
	return idx >= 0 && cleaned.slice(idx + 2).trim().length >= 3;
}

function hasSvelteIgnoreReason(lines: readonly string[], lineIndex: number): boolean {
	const line = lines[lineIndex] ?? '';
	const before = line.slice(0, line.indexOf('svelte-ignore'));
	if (/<!--\s*[^-][\s\S]{2,}-->/.test(before) || /\/\*\s*[\s\S]{3,}\*\//.test(before)) {
		return true;
	}
	for (let i = lineIndex - 1; i >= 0; i--) {
		const prev = lines[i].trim();
		if (!prev) return false;
		if (prev.includes('svelte-ignore')) continue;
		return (
			/^(?:\/\/|\/\*|<!--)/.test(prev) && prev.replace(/^[/<!\s*-]+|[-/>\s*]+$/g, '').length >= 3
		);
	}
	return false;
}

export function scanSuppressionSource(source: string, rel: string): SuppressionScanResult {
	const errors: string[] = [];
	let eslint = 0;
	let svelte = 0;

	const lines = source.split('\n');
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i];
		for (const _m of line.matchAll(INLINE_ESLINT_CONFIG_RE)) {
			void _m;
			errors.push(`${rel}:${i + 1}: inline ESLint configuration directive is forbidden`);
		}
		for (const _m of line.matchAll(BLOCK_ESLINT_DISABLE_RE)) {
			void _m;
			errors.push(
				`${rel}:${i + 1}: file-wide/block eslint-disable is forbidden; use line-scoped eslint-disable-next-line with "-- <reason>"`
			);
		}
		for (const m of line.matchAll(LINE_SCOPED_ESLINT_RE)) {
			eslint++;
			if (!hasEslintReason(m[1] ?? '')) {
				errors.push(`${rel}:${i + 1}: eslint-disable must include "-- <reason>"`);
			}
		}
		for (const _m of line.matchAll(SVELTE_IGNORE_LINE_RE)) {
			void _m;
			svelte++;
			if (!hasSvelteIgnoreReason(lines, i)) {
				errors.push(
					`${rel}:${i + 1}: svelte-ignore must have a reason comment immediately before it`
				);
			}
		}
	}

	const effect = (source.match(EFFECT_CALL_RE) ?? []).length;
	return { counts: { eslint, svelte, effect }, errors };
}
