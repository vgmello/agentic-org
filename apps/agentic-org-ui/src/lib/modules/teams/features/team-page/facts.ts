// A line of short facts separated by "·" that wraps without a separator at the start or end of a line
// (app-shell-ux §2, §6a). Each fact after the first draws its "·" in the gap before it; the list is pulled left by
// that gap and the wrapper clips it, so the dot of a fact that starts a line falls outside and is not seen. The
// wrapper's 4 px inset keeps link focus outlines inside the clip. A fact is one unit: it wraps inside itself only
// when it alone is wider than the line. Usage:
//   <div class={factsWrap}><ul role="list" class="{factsList} text-xs/5 text-gray-400"><li class={fact}>…</li></ul></div>
// The member profile's header uses the same construction (members/components/facts.ts on its branch); one shared
// helper is the lead's call once both are merged.
export const factsWrap = '-mx-1 overflow-hidden px-1 -my-1 py-1';
export const factsList = 'flex flex-wrap -ml-4';
export const fact =
	"relative min-w-0 pl-4 [overflow-wrap:anywhere] before:absolute before:left-1.5 before:font-sans before:content-['·'_/_''] before:select-none";
