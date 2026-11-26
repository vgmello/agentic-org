import type { Attachment } from 'svelte/attachments';

const HINT_RESERVE_PX = 52;
const DESKTOP_STAGE_MEDIA = '(min-width: 1280px)';

function clearFoldStyles(node: HTMLElement): void {
	const stage = node.querySelector<HTMLElement>('.home-stage');
	const belowFold = node.querySelector<HTMLElement>('.home-below-fold');
	if (stage) stage.style.minHeight = '';
	if (belowFold) belowFold.style.paddingBlockStart = '';
}

function applyFoldLayout(node: HTMLElement, scrollEl: HTMLElement, theme: string): void {
	const stage = node.querySelector<HTMLElement>('.home-stage');
	const belowFold = node.querySelector<HTMLElement>('.home-below-fold');
	if (!stage || !belowFold) {
		clearFoldStyles(node);
		return;
	}

	const viewportH = scrollEl.clientHeight;
	if (viewportH <= 0) return;

	stage.style.minHeight = '';
	belowFold.style.paddingBlockStart = '';

	const scrollRect = scrollEl.getBoundingClientRect();
	const belowRect = belowFold.getBoundingClientRect();
	const naturalTop = belowRect.top - scrollRect.top + scrollEl.scrollTop;
	const hasOverflow = naturalTop + belowRect.height > viewportH + 1;
	if (!hasOverflow) return;

	const isReport = theme.startsWith('report');
	const desktopSideBySide =
		typeof window !== 'undefined' && window.matchMedia(DESKTOP_STAGE_MEDIA).matches;
	if (!isReport && desktopSideBySide) {
		const stageRect = stage.getBoundingClientRect();
		const stageBottom = stageRect.bottom - scrollRect.top + scrollEl.scrollTop;
		const extra = viewportH - HINT_RESERVE_PX - stageBottom;
		if (extra > 0) {
			stage.style.minHeight = `${Math.round(stageRect.height + extra)}px`;
		}
	}

	const afterTop = belowFold.getBoundingClientRect().top - scrollRect.top + scrollEl.scrollTop;
	const pad = Math.max(0, Math.ceil(viewportH - afterTop));
	if (pad > 0) {
		belowFold.style.paddingBlockStart = `${pad}px`;
	}
}

/**
 * Matches POC `v6.html` `fitProjects()`: when below-the-fold sections overflow the first
 * viewport, stretches the desktop hero stage on carded themes to sit cleanly above the
 * "Scroll for more" bar and pushes `.home-below-fold` completely below the fold so Projects
 * never peeks next to the scroll hint.
 */
export function fitBelowFold(theme: string): Attachment<HTMLDivElement> {
	return (node) => {
		if (typeof window === 'undefined') return;
		const scrollEl = node.closest('main');
		if (!scrollEl) return;

		let rafId = 0;
		const run = () => applyFoldLayout(node, scrollEl, theme);
		const schedule = () => {
			cancelAnimationFrame(rafId);
			rafId = window.requestAnimationFrame(run);
		};

		const resizeObserver = new ResizeObserver(schedule);
		const observeTargets = () => {
			resizeObserver.disconnect();
			resizeObserver.observe(scrollEl);
			for (const selector of ['.home-hero', '.home-kpi-strip']) {
				const el = node.querySelector<HTMLElement>(selector);
				if (el) resizeObserver.observe(el);
			}
			const stage = node.querySelector<HTMLElement>('.home-stage');
			if (stage) {
				for (const child of stage.children) {
					resizeObserver.observe(child);
				}
			}
		};

		observeTargets();
		const mutationObserver = new MutationObserver(() => {
			observeTargets();
			schedule();
		});
		mutationObserver.observe(node, { childList: true });

		window.addEventListener('resize', schedule);
		schedule();

		return () => {
			cancelAnimationFrame(rafId);
			resizeObserver.disconnect();
			mutationObserver.disconnect();
			window.removeEventListener('resize', schedule);
			clearFoldStyles(node);
		};
	};
}
