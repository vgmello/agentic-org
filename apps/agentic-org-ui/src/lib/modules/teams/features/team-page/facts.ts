// A line of short facts separated by "·" that wraps without a separator at the start or end of a line
// (app-shell-ux §2, §6a). Each fact after the first draws its "·" in the gap before it; the list is pulled left by
// that gap and the wrapper clips it, so the dot of a fact that starts a line falls outside and is not seen. The
// wrapper's 4 px inset keeps link focus outlines inside the clip. A fact is one unit: it wraps inside itself only
// when it alone is wider than the line. Usage:
//   <div class={factsWrap}><ul role="list" class="{factsList} text-xs/5 text-fg-muted"><li class={fact}>…</li></ul></div>
// The member profile's header uses the same construction (members/components/facts.ts on its branch); one shared
// helper is the lead's call once both are merged.
// Below 640 px the clip reaches 12 px past the line, so a link's 44 px target (linkInLine) isn't cut off.
export const factsWrap = '-mx-1 overflow-hidden px-1 -my-1 py-1 max-[639px]:-my-3 max-[639px]:py-3';
export const factsList = 'flex flex-wrap -ml-4';
export const fact =
	"relative min-w-0 pl-4 [overflow-wrap:anywhere] before:absolute before:left-1.5 before:font-sans before:content-['·'_/_''] before:select-none";

/**
 * A link inside a line of facts, sized as a target (app-shell-ux §6a, SHELL-33: 24 px, 44 px below 640 px) without
 * growing the line: negative margins give back what the box adds beyond the line height. `sm` for 14/24 lines, `xs`
 * for 12/20 lines.
 */
const linkTarget =
	'inline-flex max-w-full items-center rounded-sm align-top text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere] min-h-6 max-[639px]:min-h-11';
export const linkInLine = {
	sm: `${linkTarget} max-[639px]:-my-2.5`,
	xs: `${linkTarget} -my-0.5 max-[639px]:-my-3`
} as const;
