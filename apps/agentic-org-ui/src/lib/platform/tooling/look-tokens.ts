// Look tokens: which semantic colour utility replaces each raw Tailwind palette class.
// One table for the codemod (scripts/look-tokens.ts) and the guard (look-tokens.test.ts).
// The token values live in src/app.css; README-screens.md "Look tokens" says what each is for.
// Not imported by app code.

/** The Tailwind palette families. A class naming one of these with a shade (or white/black) is raw. */
const families =
	'slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose';

/** Colour utility prefixes, longest first so `border-t` wins over `border`. */
const prefixes = [
	'ring-offset',
	'inset-ring',
	'border-x',
	'border-y',
	'border-t',
	'border-r',
	'border-b',
	'border-l',
	'border-s',
	'border-e',
	'border',
	'divide',
	'outline',
	'ring',
	'bg',
	'from',
	'via',
	'to',
	'text',
	'placeholder',
	'caret',
	'decoration',
	'fill',
	'stroke',
	'accent',
	'shadow'
];

/**
 * A raw palette class: optional negative sign, prefix, family-shade or white/black, optional opacity.
 * Groups: 1 prefix, 2 colour (`gray-400`, `white`), 3 opacity (`10`, `[0.03]`).
 * Bounded by class separators so prose words and token names (`text-fg-muted`) do not match.
 */
export function rawPaletteClass(): RegExp {
	return new RegExp(
		String.raw`(?<![\w.$/-])-?(${prefixes.join('|')})-((?:${families})-\d{2,3}|white|black)(?:\/(\d+(?:\.\d+)?|\[[^\]\s]+\]))?(?![\w/-])`,
		'g'
	);
}

type Group = 'text' | 'bg' | 'line';

function groupOf(prefix: string): Group {
	if (['text', 'placeholder', 'caret', 'decoration', 'fill', 'stroke'].includes(prefix))
		return 'text';
	if (['bg', 'from', 'via', 'to', 'accent', 'shadow'].includes(prefix)) return 'bg';
	return 'line';
}

/**
 * A tone ramp: the same step names for every status tone. `<tone>` is the base colour (icons, dots,
 * borders, tints with an opacity), `-emphasis` the solid fill, `-deep` a darker fill, and `-text`,
 * `-text-strong`, `-text-stronger` the text steps from readable to most contrast on a tint.
 */
const steps: Record<string, (tone: string) => string> = {
	'100': (tone) => `${tone}-text-stronger`,
	'200': (tone) => `${tone}-text-strong`,
	'300': (tone) => `${tone}-text`,
	'400': (tone) => tone,
	'500': (tone) => `${tone}-emphasis`,
	'600': (tone) => `${tone}-deep`
};

/** Palette family to tone, with the shades the app uses (a shade not listed is reported, not guessed). */
const toneFamilies: Array<[family: string, tone: string, shades: string[]]> = [
	['red', 'danger', ['100', '200', '300', '400', '500']],
	['amber', 'warning', ['100', '200', '300', '400', '500', '600']],
	['green', 'success', ['300', '400', '500']],
	['emerald', 'positive', ['200', '300', '400', '500']],
	['sky', 'info', ['100', '200', '300', '400', '500']]
];

const tones: Record<string, string> = Object.fromEntries(
	toneFamilies.flatMap(([family, tone, shades]) =>
		shades.map((shade) => [`${family}-${shade}`, steps[shade](tone)])
	)
);

export interface Use {
	prefix: string;
	colour: string;
	opacity?: string;
	/** The variant chain written before the utility, e.g. `hover:` or `backdrop:`. */
	variants: string;
	/** The innermost class string the class sits in (for text on a solid accent fill). */
	context: string;
}

/** A solid emphasis fill in the same class string: its text is `on-emphasis`, not page text. */
const emphasisFill =
	/(?:^|[\s"'`])(?:bg-(?:indigo|red)-(?:500|600)|bg-primary|bg-danger-emphasis)(?=$|[\s"'`])/;

/**
 * The token utility for one raw use, or undefined when the table has no role for it.
 * Returns the full replacement utility without variants (`bg-surface-raised`, `bg-danger-emphasis/10`).
 */
export function tokenFor(use: Use): string | undefined {
	const { prefix, colour, opacity, variants, context } = use;
	const group = groupOf(prefix);
	const op = opacity ? `/${opacity}` : '';
	const keep = (token: string | undefined) => (token ? `${prefix}-${token}${op}` : undefined);
	switch (colour) {
		case 'white':
			if (group === 'text') return keep(emphasisFill.test(context) ? 'on-emphasis' : 'fg');
			if (group === 'bg') {
				if (!opacity) return `${prefix}-on-emphasis`;
				if (opacity === '5') return `${prefix}-surface-raised`;
				if (opacity === '10') return `${prefix}-surface-overlay`;
				return keep('tint');
			}
			if (!opacity) return `${prefix}-fg`;
			if (opacity === '5') return `${prefix}-line-subtle`;
			if (opacity === '10') return `${prefix}-line`;
			if (opacity === '20') return `${prefix}-line-strong`;
			return keep('tint');
		case 'black':
			return keep('shade');
		case 'gray-100':
			return group === 'text' ? keep('fg-bright') : undefined;
		case 'gray-200':
			return group === 'text' ? keep('fg-body') : undefined;
		case 'gray-300':
			return group === 'text' ? keep('fg-secondary') : undefined;
		case 'gray-400':
			return keep(group === 'text' ? 'fg-muted' : 'quiet');
		case 'gray-500':
			return keep(
				group === 'text' ? 'fg-subtle' : group === 'bg' ? 'quiet-emphasis' : 'field-border'
			);
		case 'gray-700':
			return group === 'bg' ? keep('surface-chip') : undefined;
		case 'gray-800':
			return group === 'text' ? undefined : keep('surface');
		case 'gray-900':
			return keep(group === 'text' ? 'on-warning' : 'surface-sunken');
		case 'gray-950':
			if (group === 'text') return keep('on-warning-strong');
			return keep(/(?:^|:)backdrop:$/.test(variants) ? 'scrim' : 'tooltip');
		case 'indigo-100':
			return keep('brand-text-stronger');
		case 'indigo-200':
			return keep('brand-text-strong');
		case 'indigo-300':
			return keep('brand-text');
		case 'indigo-400':
			if (group === 'text') return keep('link');
			if (prefix === 'outline' && !opacity) return keep('focus');
			return keep('brand');
		case 'indigo-500':
			if ((prefix === 'outline' || prefix === 'ring') && !opacity) return keep('ring');
			return keep('primary');
		case 'indigo-600':
			return group === 'bg' ? keep('primary-hover') : undefined;
		default:
			return keep(tones[colour]);
	}
}

/**
 * Per-file overrides for a class whose role the rules above cannot tell: `{ file: { class: replacement } }`,
 * matched on the whole class as written (with variants). Empty today: every use maps by its prefix.
 */
export const overrides: Record<string, Record<string, string>> = {};

/** Files allowed to name raw palette colours, each with its reason. */
export const allowlist: Record<string, string> = {
	'src/app.css': 'The token definitions: the only place palette values are named.',
	'src/lib/styles/themes/base.css': 'Shared executive theme fallback tokens and surfaces.',
	'src/lib/styles/themes/ocean.css': 'Ocean Dark theme token definitions and canvas palette.',
	'src/lib/styles/themes/classic.css': 'Classic Slate theme token definitions and flat surface palette.',
	'src/lib/styles/themes/report.css': 'Report (Modern 2D Executive Day) theme tokens and editorial layout.',
	'src/lib/styles/themes/report-dark.css': 'Report Night (Modern 2D Executive Dark) theme tokens and palette.',
	'src/lib/platform/tooling/look-tokens.ts': 'This mapping table.',
	'src/lib/platform/tooling/look-tokens.test.ts': 'The guard and its fixtures.'
};

export interface Change {
	line: number;
	from: string;
	to: string;
}

export interface Rewrite {
	text: string;
	changes: Change[];
	unmapped: Array<{ line: number; class: string }>;
}

function lineAt(text: string, index: number) {
	let line = 1;
	for (let i = 0; i < index; i++) if (text.charCodeAt(i) === 10) line++;
	return line;
}

/** The variant chain just before `index` (back to the previous separator), e.g. `hover:`. */
function variantsBefore(text: string, index: number) {
	let start = index;
	while (start > 0 && !/[\s"'`{(,]/.test(text[start - 1])) start--;
	return text.slice(start, index);
}

/** The innermost quoted string around `index` on its line. */
function contextAround(text: string, index: number) {
	const lineStart = text.lastIndexOf('\n', index) + 1;
	const lineEnd = text.indexOf('\n', index);
	const end = lineEnd < 0 ? text.length : lineEnd;
	for (let i = index - 1; i >= lineStart; i--) {
		const quote = text[i];
		if (quote === '"' || quote === "'" || quote === '`') {
			const close = text.indexOf(quote, index);
			if (close >= 0 && close <= end) return text.slice(i + 1, close);
		}
	}
	return text.slice(lineStart, end);
}

/** Rewrites every mappable raw palette class in `text`; reports what it could not map. */
export function rewrite(text: string, file: string): Rewrite {
	const changes: Change[] = [];
	const unmapped: Rewrite['unmapped'] = [];
	const fileOverrides = overrides[file] ?? {};
	const out = text.replace(
		rawPaletteClass(),
		(match, prefix: string, colour: string, opacity: string | undefined, offset: number) => {
			const variants = variantsBefore(text, offset);
			const negative = match.startsWith('-') ? '-' : '';
			const override = fileOverrides[variants + match];
			const token =
				override ??
				tokenFor({ prefix, colour, opacity, variants, context: contextAround(text, offset) });
			if (!token) {
				unmapped.push({ line: lineAt(text, offset), class: variants + match });
				return match;
			}
			const to = override ? override.slice(variants.length) : negative + token;
			changes.push({ line: lineAt(text, offset), from: match, to });
			return to;
		}
	);
	return { text: out, changes, unmapped };
}

/** Raw palette classes left in `text`, with the token to use instead when there is one. */
export function findRaw(text: string): Array<{ line: number; class: string; use?: string }> {
	const found: Array<{ line: number; class: string; use?: string }> = [];
	for (const m of text.matchAll(rawPaletteClass())) {
		const offset = m.index ?? 0;
		const variants = variantsBefore(text, offset);
		const use = tokenFor({
			prefix: m[1],
			colour: m[2],
			opacity: m[3],
			variants,
			context: contextAround(text, offset)
		});
		found.push({
			line: lineAt(text, offset),
			class: variants + m[0],
			use: use ? variants + use : undefined
		});
	}
	return found;
}
