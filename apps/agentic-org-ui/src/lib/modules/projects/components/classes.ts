// Class strings the Projects screens share. Colours come from look tokens only (README-screens.md "Look tokens").

/**
 * A link inside a line of text, sized as a target (app-shell-ux §6a: 24 px, 44 px below 640 px) without growing the
 * line: negative margins give back what the box adds beyond the line height. `sm` for 14/24 lines, `xs` for 12/20.
 */
const linkTarget =
	'relative inline-flex max-w-full items-center rounded-sm align-top text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere] min-h-6 max-[639px]:min-h-11';
export const inlineLink = {
	sm: `${linkTarget} max-[639px]:-my-2.5`,
	xs: `${linkTarget} -my-0.5 max-[639px]:-my-3`
} as const;

/** A line of facts: each fact one unit that wraps as a whole, spaced rather than dotted so no separator strands. */
export const factsLine = 'flex flex-wrap items-baseline gap-x-5 gap-y-1';
export const fact = 'min-w-0 [overflow-wrap:anywhere]';

/** The stretched title link of a divided row. */
export const rowTitleLink =
	'rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]';
