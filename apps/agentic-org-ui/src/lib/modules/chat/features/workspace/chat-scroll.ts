import { tick } from 'svelte';

export interface FeedScrollState {
	hasNewer: boolean;
	unseenCount: number;
}

const BOUNDARY_GAP = 8;

/**
 * Syncs sticky older-day headers (`.chat-day:not(.is-today)`):
 * - `.is-stuck` adds a solid translucent band when pinned at the top of `listEl`.
 * - `.is-past` hides older pinned headers once the unlabelled Today divider reaches the top.
 */
export function syncStuckDayHeaders(listEl: HTMLElement): void {
	const top = listEl.getBoundingClientRect().top;
	const today = listEl.querySelector<HTMLElement>('.chat-day.is-today');
	const inToday = today !== null && today.getBoundingClientRect().top - top < 1;
	const older = [...listEl.querySelectorAll<HTMLElement>('.chat-day:not(.is-today)')];
	let stuckIdx = -1;
	if (listEl.scrollTop > 0 && !inToday) {
		for (let i = 0; i < older.length; i += 1) {
			if (older[i].getBoundingClientRect().top - top < 1) stuckIdx = i;
		}
	}
	for (let i = 0; i < older.length; i += 1) {
		older[i].classList.toggle('is-stuck', i === stuckIdx);
		older[i].classList.toggle('is-past', inToday || (stuckIdx >= 0 && i < stuckIdx));
	}
}

/**
 * Computes whether messages extend below the visible fold of `listEl`,
 * plus the count of messages wholly below the fold (`N newer messages` vs `Jump to latest`).
 */
export function computeNewerBelow(listEl: HTMLElement): FeedScrollState {
	const msgs = listEl.querySelectorAll<HTMLElement>('[data-testid="message"]');
	const last = msgs[msgs.length - 1];
	if (!last) return { hasNewer: false, unseenCount: 0 };
	const bottom = listEl.getBoundingClientRect().bottom;
	const hasNewer = last.getBoundingClientRect().bottom > bottom + 1;
	if (!hasNewer) return { hasNewer: false, unseenCount: 0 };
	let unseenCount = 0;
	for (const m of msgs) {
		if (m.getBoundingClientRect().top >= bottom - 1) unseenCount += 1;
	}
	return { hasNewer: true, unseenCount };
}

/**
 * Positions a deep-linked or pinned message cleanly below any sticky day header band
 * (roughly one-third down the viewport, or just below the sticky day band if tall).
 */
export function scrollMessageBelowStickyHeader(
	listEl: HTMLElement | (() => HTMLElement | undefined) | undefined,
	messageId: string | undefined
): void {
	if (!messageId) return;
	void tick().then(() => {
		const initialList = typeof listEl === 'function' ? listEl() : listEl;
		const scope: ParentNode = initialList ?? document;
		const el = scope.querySelector<HTMLElement>(`[data-id="${CSS.escape(messageId)}"]`);
		if (!el) return;
		const container =
			initialList ?? el.closest<HTMLElement>('[data-testid="message-list"]') ?? undefined;
		if (!container || container.clientHeight <= 0) {
			el.scrollIntoView({ block: 'center' });
			return;
		}
		const band =
			container.querySelector<HTMLElement>('.chat-day:not(.is-today)')?.offsetHeight || 0;
		const elTop =
			el.getBoundingClientRect().top - container.getBoundingClientRect().top + container.scrollTop;
		const desiredOffset = Math.max(
			BOUNDARY_GAP + band,
			(container.clientHeight - el.offsetHeight) / 3
		);
		container.scrollTop = Math.max(0, Math.round(elTop - desiredOffset));
		syncStuckDayHeaders(container);
	});
}

/**
 * Scrolls to the latest message, syncs stuck day headers, and moves temporary focus
 * to the newest message so keyboard users are not dropped to `document.body`.
 */
export function jumpToLatestMessage(listEl: HTMLElement | undefined): void {
	if (!listEl) return;
	listEl.scrollTop = listEl.scrollHeight;
	syncStuckDayHeaders(listEl);
	const msgs = listEl.querySelectorAll<HTMLElement>('[data-testid="message"]');
	const last = msgs[msgs.length - 1];
	if (!last) return;
	const temporary = last.tabIndex < 0 && !last.hasAttribute('tabindex');
	if (temporary) last.tabIndex = -1;
	last.focus({ preventScroll: true });
	if (temporary) {
		const clear = () => {
			if (last.getAttribute('tabindex') === '-1') last.removeAttribute('tabindex');
		};
		if (document.activeElement === last) last.addEventListener('blur', clear, { once: true });
		else clear();
	}
}

export interface FeedScrollActionParams {
	active: boolean;
	version: string;
	onState: (state: FeedScrollState) => void;
}

/**
 * Svelte action tracking sticky day headers and below-fold unread/newer messages.
 * Cancels any pending `requestAnimationFrame` on unmount (zero `$effect`).
 */
export function trackFeedScroll(node: HTMLElement, initial: FeedScrollActionParams) {
	let params = initial;
	let rafId: number | undefined;
	let lastState: FeedScrollState | undefined;

	const emit = (next: FeedScrollState) => {
		if (
			lastState &&
			lastState.hasNewer === next.hasNewer &&
			lastState.unseenCount === next.unseenCount
		)
			return;
		lastState = next;
		params.onState(next);
	};

	const measure = () => {
		rafId = undefined;
		if (!node.isConnected) return;
		if (!params.active) {
			emit({ hasNewer: false, unseenCount: 0 });
			return;
		}
		syncStuckDayHeaders(node);
		emit(computeNewerBelow(node));
	};

	const schedule = () => {
		if (rafId !== undefined) cancelAnimationFrame(rafId);
		rafId = requestAnimationFrame(measure);
	};

	const onScroll = () => schedule();
	node.addEventListener('scroll', onScroll, { passive: true });

	let ro: ResizeObserver | undefined;
	if (typeof ResizeObserver !== 'undefined') {
		ro = new ResizeObserver(schedule);
		ro.observe(node);
	}
	schedule();

	return {
		update(next: FeedScrollActionParams) {
			params = next;
			schedule();
		},
		destroy() {
			if (rafId !== undefined) {
				cancelAnimationFrame(rafId);
				rafId = undefined;
			}
			node.removeEventListener('scroll', onScroll);
			ro?.disconnect();
		}
	};
}
