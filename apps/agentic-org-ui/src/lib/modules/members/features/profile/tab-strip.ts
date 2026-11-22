// The profile's tab strip from 640 px up (app-shell-ux §6a): nine tabs can overflow the strip, which scrolls inside
// itself. So the current tab is brought into view on load and after each move, a tab that gets keyboard focus is
// centred, and an edge that hides more tabs fades out (`data-edge` on the wrapper), so the person can see there is
// more. Centring keeps the focused or current tab clear of the faded edge.
import type { Attachment } from 'svelte/attachments';

export type Edge = '' | 'left' | 'right' | 'both';

/** Which edges hide tabs, from the strip's scroll position. */
export function hiddenEdges(scrollLeft: number, clientWidth: number, scrollWidth: number): Edge {
	const left = scrollLeft > 1;
	const right = scrollLeft + clientWidth < scrollWidth - 1;
	return left && right ? 'both' : left ? 'left' : right ? 'right' : '';
}

/** `{@attach tabStrip(page.url.pathname)}` on an element that wraps the strip's `nav`. */
export const tabStrip =
	(path: string): Attachment<HTMLElement> =>
	(wrap) => {
		const nav = wrap.querySelector('nav');
		if (!nav) return;
		const centre = (el: HTMLElement) => {
			const delta = el.getBoundingClientRect().left - nav.getBoundingClientRect().left;
			nav.scrollLeft += delta - (nav.clientWidth - el.offsetWidth) / 2;
		};
		const update = () => {
			wrap.dataset.edge = hiddenEdges(nav.scrollLeft, nav.clientWidth, nav.scrollWidth);
		};
		const focused = (event: FocusEvent) => {
			if (event.target instanceof HTMLElement && event.target !== nav) centre(event.target);
		};
		const current = nav.querySelector<HTMLElement>('[aria-current="page"]');
		// `path` is read only so a move re-runs this and the new current tab is centred.
		if (current && path) centre(current);
		update();
		nav.addEventListener('scroll', update, { passive: true });
		nav.addEventListener('focusin', focused);
		const observer = new ResizeObserver(update);
		observer.observe(nav);
		return () => {
			nav.removeEventListener('scroll', update);
			nav.removeEventListener('focusin', focused);
			observer.disconnect();
		};
	};

/** The fade on the wrapper's `nav`, per hidden edge. */
export const tabStripFade =
	'data-[edge=right]:[&>nav]:[mask-image:linear-gradient(to_left,transparent,black_3rem)] data-[edge=left]:[&>nav]:[mask-image:linear-gradient(to_right,transparent,black_3rem)] data-[edge=both]:[&>nav]:[mask-image:linear-gradient(to_right,transparent,black_3rem,black_calc(100%_-_3rem),transparent)]';
